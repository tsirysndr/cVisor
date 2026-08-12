# Build the cVisor Erlang NIF.
#
# Compiles c_src/cvisor_nif.c against erl_nif.h into priv/cvisor_nif.so and
# compiles the Erlang module. Requires an Erlang/OTP install (erl on PATH) and
# a C compiler. libcvisor.so must be present in priv/ (copied by
# `cargo xtask ffi` from the repo root) or reachable via CVISOR_LIB.

ERL_ROOT := $(shell erl -noshell -eval 'io:format("~s", [code:root_dir()])' -s init stop)
ERTS_INCLUDE := $(shell erl -noshell -eval 'io:format("~s/erts-~s/include", [code:root_dir(), erlang:system_info(version)])' -s init stop)

CC ?= cc
CFLAGS ?= -O2 -fPIC -Wall
LDFLAGS ?= -shared -ldl

PRIV := priv
EBIN := ebin

.PHONY: all clean

all: $(PRIV)/cvisor_nif.so $(EBIN)/cvisor.beam

$(PRIV)/cvisor_nif.so: c_src/cvisor_nif.c
	@mkdir -p $(PRIV)
	$(CC) $(CFLAGS) -I"$(ERTS_INCLUDE)" $(LDFLAGS) -o $@ $<

$(EBIN)/cvisor.beam: src/cvisor.erl
	@mkdir -p $(EBIN)
	erlc -o $(EBIN) $<

clean:
	rm -f $(PRIV)/cvisor_nif.so $(EBIN)/cvisor.beam
