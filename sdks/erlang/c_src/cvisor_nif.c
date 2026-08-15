/* cVisor Erlang NIF — bridges the BEAM to the libcvisor C ABI.
 *
 * Exposes cvisor:run_nif/2, which creates a sandbox, runs a command to
 * completion (optionally SIGKILLed after a timeout), and returns
 * {ok, Stdout, Stderr, ExitCode} | {error, Reason}, plus
 * cvisor:set_allow_network_nif/1 which toggles outbound networking for
 * subsequently created sandboxes. libcvisor.so is dlopen'd at load time
 * (CVISOR_LIB env override, else the NIF's own directory), so there is no
 * link-time dependency on it.
 */

#include <erl_nif.h>
#include <dlfcn.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef void *(*fn_new)(void);
typedef void (*fn_free)(void *);
typedef void *(*fn_run_timeout)(void *, const char *, uint64_t);
typedef void (*fn_out_free)(void *);
typedef uint8_t *(*fn_out_get)(void *, size_t *);
typedef int (*fn_out_exit_code)(void *);
typedef void (*fn_bytes_free)(uint8_t *, size_t);
typedef void (*fn_set_allow_network)(void *, int);

typedef void *(*fn_session_start)(void *, const char *, int);
typedef uint8_t *(*fn_session_read)(void *, size_t *);
typedef long (*fn_session_write)(void *, const uint8_t *, size_t);
typedef void (*fn_session_resize)(void *, uint16_t, uint16_t);
typedef int (*fn_session_try_wait)(void *, int *);
typedef int (*fn_session_wait)(void *);
typedef void (*fn_session_kill)(void *);
typedef void (*fn_session_free)(void *);

static void *g_lib = NULL;
static fn_new p_new;
static fn_free p_free;
static fn_run_timeout p_run_timeout;
static fn_out_free p_out_free;
static fn_out_get p_stdout;
static fn_out_get p_stderr;
static fn_out_exit_code p_exit_code;
static fn_bytes_free p_bytes_free;
static fn_set_allow_network p_set_allow_network;

static fn_session_start p_session_start;
static fn_session_read p_session_read_stdout;
static fn_session_read p_session_read_stderr;
static fn_session_write p_session_write_stdin;
static fn_session_resize p_session_resize;
static fn_session_try_wait p_session_try_wait;
static fn_session_wait p_session_wait;
static fn_session_kill p_session_kill;
static fn_session_free p_session_free;

/* Resource type for a live streaming session; holds the sandbox + session. */
static ErlNifResourceType *SESSION_RES = NULL;

typedef struct {
    void *sb;
    void *sess;
    int freed;
} session_t;

/* Applied to each sandbox before running; default allow (matches libcvisor). */
static int g_allow_network = 1;

static int load_lib(void) {
    const char *path = getenv("CVISOR_LIB");
    if (!path || !path[0]) {
        path = "libcvisor.so"; /* rely on rpath/priv or LD_LIBRARY_PATH */
    }
    g_lib = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (!g_lib) return 0;

    p_new = (fn_new)dlsym(g_lib, "cvisor_sandbox_new");
    p_free = (fn_free)dlsym(g_lib, "cvisor_sandbox_free");
    p_run_timeout = (fn_run_timeout)dlsym(g_lib, "cvisor_run_timeout");
    p_out_free = (fn_out_free)dlsym(g_lib, "cvisor_output_free");
    p_stdout = (fn_out_get)dlsym(g_lib, "cvisor_output_stdout");
    p_stderr = (fn_out_get)dlsym(g_lib, "cvisor_output_stderr");
    p_exit_code = (fn_out_exit_code)dlsym(g_lib, "cvisor_output_exit_code");
    p_bytes_free = (fn_bytes_free)dlsym(g_lib, "cvisor_bytes_free");
    p_set_allow_network =
        (fn_set_allow_network)dlsym(g_lib, "cvisor_sandbox_set_allow_network");

    p_session_start = (fn_session_start)dlsym(g_lib, "cvisor_session_start");
    p_session_read_stdout =
        (fn_session_read)dlsym(g_lib, "cvisor_session_read_stdout");
    p_session_read_stderr =
        (fn_session_read)dlsym(g_lib, "cvisor_session_read_stderr");
    p_session_write_stdin =
        (fn_session_write)dlsym(g_lib, "cvisor_session_write_stdin");
    p_session_resize = (fn_session_resize)dlsym(g_lib, "cvisor_session_resize");
    p_session_try_wait =
        (fn_session_try_wait)dlsym(g_lib, "cvisor_session_try_wait");
    p_session_wait = (fn_session_wait)dlsym(g_lib, "cvisor_session_wait");
    p_session_kill = (fn_session_kill)dlsym(g_lib, "cvisor_session_kill");
    p_session_free = (fn_session_free)dlsym(g_lib, "cvisor_session_free");

    return p_new && p_free && p_run_timeout && p_out_free && p_stdout &&
           p_stderr && p_exit_code && p_bytes_free && p_set_allow_network &&
           p_session_start && p_session_read_stdout && p_session_read_stderr &&
           p_session_write_stdin && p_session_resize && p_session_try_wait &&
           p_session_wait && p_session_kill && p_session_free;
}

static ERL_NIF_TERM make_bin(ErlNifEnv *env, fn_out_get get, void *out) {
    size_t len = 0;
    uint8_t *data = get(out, &len);
    ErlNifBinary bin;
    enif_alloc_binary(len, &bin);
    if (len > 0 && data) {
        memcpy(bin.data, data, len);
    }
    if (data) {
        p_bytes_free(data, len);
    }
    return enif_make_binary(env, &bin);
}

static ERL_NIF_TERM run_nif(ErlNifEnv *env, int argc,
                            const ERL_NIF_TERM argv[]) {
    ErlNifBinary cmd_bin;
    ErlNifUInt64 timeout_ms;
    if (argc != 2 || !enif_inspect_binary(env, argv[0], &cmd_bin) ||
        !enif_get_uint64(env, argv[1], &timeout_ms)) {
        return enif_make_badarg(env);
    }

    /* NUL-terminate the command. */
    char *cmd = enif_alloc(cmd_bin.size + 1);
    if (!cmd) {
        return enif_raise_exception(env, enif_make_atom(env, "enomem"));
    }
    memcpy(cmd, cmd_bin.data, cmd_bin.size);
    cmd[cmd_bin.size] = '\0';

    void *sb = p_new();
    if (!sb) {
        enif_free(cmd);
        return enif_make_tuple2(env, enif_make_atom(env, "error"),
                                enif_make_atom(env, "sandbox_new_failed"));
    }
    p_set_allow_network(sb, g_allow_network);

    void *out = p_run_timeout(sb, cmd, (uint64_t)timeout_ms);
    enif_free(cmd);
    if (!out) {
        p_free(sb);
        return enif_make_tuple2(env, enif_make_atom(env, "error"),
                                enif_make_atom(env, "run_failed"));
    }

    ERL_NIF_TERM stdout_term = make_bin(env, p_stdout, out);
    ERL_NIF_TERM stderr_term = make_bin(env, p_stderr, out);
    int exit_code = p_exit_code(out);

    p_out_free(out);
    p_free(sb);

    return enif_make_tuple4(env, enif_make_atom(env, "ok"), stdout_term,
                            stderr_term, enif_make_int(env, exit_code));
}

static ERL_NIF_TERM set_allow_network_nif(ErlNifEnv *env, int argc,
                                          const ERL_NIF_TERM argv[]) {
    int allow;
    if (argc != 1 || !enif_get_int(env, argv[0], &allow)) {
        return enif_make_badarg(env);
    }
    g_allow_network = allow ? 1 : 0;
    return enif_make_atom(env, "ok");
}

/* Free the session then its sandbox; idempotent via the freed flag. */
static void session_close(session_t *s) {
    if (s->freed) {
        return;
    }
    if (s->sess) {
        p_session_free(s->sess);
        s->sess = NULL;
    }
    if (s->sb) {
        p_free(s->sb);
        s->sb = NULL;
    }
    s->freed = 1;
}

static void session_dtor(ErlNifEnv *env, void *obj) {
    (void)env;
    session_close((session_t *)obj);
}

static int get_session(ErlNifEnv *env, ERL_NIF_TERM term, session_t **out) {
    return enif_get_resource(env, term, SESSION_RES, (void **)out);
}

static ERL_NIF_TERM session_start_nif(ErlNifEnv *env, int argc,
                                      const ERL_NIF_TERM argv[]) {
    ErlNifBinary cmd_bin;
    int pty;
    if (argc != 2 || !enif_inspect_binary(env, argv[0], &cmd_bin) ||
        !enif_get_int(env, argv[1], &pty)) {
        return enif_make_badarg(env);
    }

    char *cmd = NULL;
    /* PTY sessions always run /bin/sh -i; ignore any provided command. */
    if (!pty) {
        cmd = enif_alloc(cmd_bin.size + 1);
        if (!cmd) {
            return enif_raise_exception(env, enif_make_atom(env, "enomem"));
        }
        memcpy(cmd, cmd_bin.data, cmd_bin.size);
        cmd[cmd_bin.size] = '\0';
    }

    void *sb = p_new();
    if (!sb) {
        if (cmd) {
            enif_free(cmd);
        }
        return enif_make_tuple2(env, enif_make_atom(env, "error"),
                                enif_make_atom(env, "sandbox_new_failed"));
    }
    p_set_allow_network(sb, g_allow_network);

    void *sess = p_session_start(sb, cmd, pty);
    if (cmd) {
        enif_free(cmd);
    }
    if (!sess) {
        p_free(sb);
        return enif_make_tuple2(env, enif_make_atom(env, "error"),
                                enif_make_atom(env, "session_start_failed"));
    }

    session_t *res = enif_alloc_resource(SESSION_RES, sizeof(session_t));
    if (!res) {
        p_session_free(sess);
        p_free(sb);
        return enif_make_tuple2(env, enif_make_atom(env, "error"),
                                enif_make_atom(env, "alloc_failed"));
    }
    res->sb = sb;
    res->sess = sess;
    res->freed = 0;

    ERL_NIF_TERM term = enif_make_resource(env, res);
    enif_release_resource(res);
    return enif_make_tuple2(env, enif_make_atom(env, "ok"), term);
}

static ERL_NIF_TERM session_read_stdout_nif(ErlNifEnv *env, int argc,
                                            const ERL_NIF_TERM argv[]) {
    session_t *s;
    if (argc != 1 || !get_session(env, argv[0], &s)) {
        return enif_make_badarg(env);
    }
    if (s->freed) {
        return enif_make_badarg(env);
    }
    return make_bin(env, p_session_read_stdout, s->sess);
}

static ERL_NIF_TERM session_read_stderr_nif(ErlNifEnv *env, int argc,
                                            const ERL_NIF_TERM argv[]) {
    session_t *s;
    if (argc != 1 || !get_session(env, argv[0], &s)) {
        return enif_make_badarg(env);
    }
    if (s->freed) {
        return enif_make_badarg(env);
    }
    return make_bin(env, p_session_read_stderr, s->sess);
}

static ERL_NIF_TERM session_write_stdin_nif(ErlNifEnv *env, int argc,
                                            const ERL_NIF_TERM argv[]) {
    session_t *s;
    ErlNifBinary data;
    if (argc != 2 || !get_session(env, argv[0], &s) ||
        !enif_inspect_binary(env, argv[1], &data)) {
        return enif_make_badarg(env);
    }
    if (s->freed) {
        return enif_make_badarg(env);
    }
    long n = p_session_write_stdin(s->sess, data.data, data.size);
    return enif_make_long(env, n);
}

static ERL_NIF_TERM session_resize_nif(ErlNifEnv *env, int argc,
                                       const ERL_NIF_TERM argv[]) {
    session_t *s;
    unsigned int rows, cols;
    if (argc != 3 || !get_session(env, argv[0], &s) ||
        !enif_get_uint(env, argv[1], &rows) ||
        !enif_get_uint(env, argv[2], &cols)) {
        return enif_make_badarg(env);
    }
    if (s->freed) {
        return enif_make_badarg(env);
    }
    p_session_resize(s->sess, (uint16_t)rows, (uint16_t)cols);
    return enif_make_atom(env, "ok");
}

static ERL_NIF_TERM session_try_wait_nif(ErlNifEnv *env, int argc,
                                         const ERL_NIF_TERM argv[]) {
    session_t *s;
    if (argc != 1 || !get_session(env, argv[0], &s)) {
        return enif_make_badarg(env);
    }
    if (s->freed) {
        return enif_make_badarg(env);
    }
    int done = 0;
    int code = p_session_try_wait(s->sess, &done);
    if (!done) {
        return enif_make_atom(env, "running");
    }
    return enif_make_tuple2(env, enif_make_atom(env, "done"),
                            enif_make_int(env, code));
}

static ERL_NIF_TERM session_wait_nif(ErlNifEnv *env, int argc,
                                     const ERL_NIF_TERM argv[]) {
    session_t *s;
    if (argc != 1 || !get_session(env, argv[0], &s)) {
        return enif_make_badarg(env);
    }
    if (s->freed) {
        return enif_make_badarg(env);
    }
    int code = p_session_wait(s->sess);
    return enif_make_int(env, code);
}

static ERL_NIF_TERM session_kill_nif(ErlNifEnv *env, int argc,
                                     const ERL_NIF_TERM argv[]) {
    session_t *s;
    if (argc != 1 || !get_session(env, argv[0], &s)) {
        return enif_make_badarg(env);
    }
    if (s->freed) {
        return enif_make_badarg(env);
    }
    p_session_kill(s->sess);
    return enif_make_atom(env, "ok");
}

static ERL_NIF_TERM session_free_nif(ErlNifEnv *env, int argc,
                                     const ERL_NIF_TERM argv[]) {
    session_t *s;
    if (argc != 1 || !get_session(env, argv[0], &s)) {
        return enif_make_badarg(env);
    }
    session_close(s);
    return enif_make_atom(env, "ok");
}

static int on_load(ErlNifEnv *env, void **priv_data, ERL_NIF_TERM load_info) {
    (void)priv_data;
    (void)load_info;
    SESSION_RES = enif_open_resource_type(env, NULL, "cvisor_session",
                                          session_dtor, ERL_NIF_RT_CREATE, NULL);
    if (!SESSION_RES) {
        return -1;
    }
    return load_lib() ? 0 : -1;
}

static ErlNifFunc nif_funcs[] = {
    /* Marked dirty (I/O bound): a run blocks until the guest exits. */
    {"run_nif", 2, run_nif, ERL_NIF_DIRTY_JOB_IO_BOUND},
    {"set_allow_network_nif", 1, set_allow_network_nif, 0},
    {"session_start_nif", 2, session_start_nif, 0},
    {"session_read_stdout_nif", 1, session_read_stdout_nif, 0},
    {"session_read_stderr_nif", 1, session_read_stderr_nif, 0},
    {"session_write_stdin_nif", 2, session_write_stdin_nif, 0},
    {"session_resize_nif", 3, session_resize_nif, 0},
    {"session_try_wait_nif", 1, session_try_wait_nif, 0},
    /* Marked dirty (I/O bound): blocks until the session exits. */
    {"session_wait_nif", 1, session_wait_nif, ERL_NIF_DIRTY_JOB_IO_BOUND},
    {"session_kill_nif", 1, session_kill_nif, 0},
    {"session_free_nif", 1, session_free_nif, 0},
};

ERL_NIF_INIT(cvisor, nif_funcs, on_load, NULL, NULL, NULL)
