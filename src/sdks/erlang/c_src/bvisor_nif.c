/* bVisor Erlang NIF — bridges the BEAM to the libbvisor C ABI.
 *
 * Exposes a single NIF, bvisor:run_nif/1, which creates a sandbox, runs a
 * command to completion, and returns {ok, Stdout, Stderr} | {error, Reason}.
 * libbvisor.so is dlopen'd at load time (BVISOR_LIB env override, else the
 * NIF's own directory), so there is no link-time dependency on it.
 */

#include <erl_nif.h>
#include <dlfcn.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef void *(*fn_new)(void);
typedef void (*fn_free)(void *);
typedef void *(*fn_run)(void *, const char *);
typedef void (*fn_out_free)(void *);
typedef uint8_t *(*fn_out_get)(void *, size_t *);
typedef void (*fn_bytes_free)(uint8_t *, size_t);

static void *g_lib = NULL;
static fn_new p_new;
static fn_free p_free;
static fn_run p_run;
static fn_out_free p_out_free;
static fn_out_get p_stdout;
static fn_out_get p_stderr;
static fn_bytes_free p_bytes_free;

static int load_lib(void) {
    const char *path = getenv("BVISOR_LIB");
    if (!path || !path[0]) {
        path = "libbvisor.so"; /* rely on rpath/priv or LD_LIBRARY_PATH */
    }
    g_lib = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (!g_lib) return 0;

    p_new = (fn_new)dlsym(g_lib, "bvisor_sandbox_new");
    p_free = (fn_free)dlsym(g_lib, "bvisor_sandbox_free");
    p_run = (fn_run)dlsym(g_lib, "bvisor_run");
    p_out_free = (fn_out_free)dlsym(g_lib, "bvisor_output_free");
    p_stdout = (fn_out_get)dlsym(g_lib, "bvisor_output_stdout");
    p_stderr = (fn_out_get)dlsym(g_lib, "bvisor_output_stderr");
    p_bytes_free = (fn_bytes_free)dlsym(g_lib, "bvisor_bytes_free");

    return p_new && p_free && p_run && p_out_free && p_stdout && p_stderr &&
           p_bytes_free;
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
    if (argc != 1 || !enif_inspect_binary(env, argv[0], &cmd_bin)) {
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

    void *out = p_run(sb, cmd);
    enif_free(cmd);
    if (!out) {
        p_free(sb);
        return enif_make_tuple2(env, enif_make_atom(env, "error"),
                                enif_make_atom(env, "run_failed"));
    }

    ERL_NIF_TERM stdout_term = make_bin(env, p_stdout, out);
    ERL_NIF_TERM stderr_term = make_bin(env, p_stderr, out);

    p_out_free(out);
    p_free(sb);

    return enif_make_tuple3(env, enif_make_atom(env, "ok"), stdout_term,
                            stderr_term);
}

static int on_load(ErlNifEnv *env, void **priv_data, ERL_NIF_TERM load_info) {
    (void)env;
    (void)priv_data;
    (void)load_info;
    return load_lib() ? 0 : -1;
}

static ErlNifFunc nif_funcs[] = {
    /* Marked dirty (I/O bound): a run blocks until the guest exits. */
    {"run_nif", 1, run_nif, ERL_NIF_DIRTY_JOB_IO_BOUND},
};

ERL_NIF_INIT(bvisor, nif_funcs, on_load, NULL, NULL, NULL)
