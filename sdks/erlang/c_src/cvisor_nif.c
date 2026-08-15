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

    return p_new && p_free && p_run_timeout && p_out_free && p_stdout &&
           p_stderr && p_exit_code && p_bytes_free && p_set_allow_network;
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

static int on_load(ErlNifEnv *env, void **priv_data, ERL_NIF_TERM load_info) {
    (void)env;
    (void)priv_data;
    (void)load_info;
    return load_lib() ? 0 : -1;
}

static ErlNifFunc nif_funcs[] = {
    /* Marked dirty (I/O bound): a run blocks until the guest exits. */
    {"run_nif", 2, run_nif, ERL_NIF_DIRTY_JOB_IO_BOUND},
    {"set_allow_network_nif", 1, set_allow_network_nif, 0},
};

ERL_NIF_INIT(cvisor, nif_funcs, on_load, NULL, NULL, NULL)
