# Build the bVisor Erlang NIF.
#
# Compiles c_src/bvisor_nif.c against erl_nif.h into priv/bvisor_nif.so and
# compiles the Erlang module. Requires an Erlang/OTP install (erl on PATH) and
# a C compiler. libbvisor.so must be present in priv/ (copied by
# `cargo xtask ffi` from the repo root) or reachable via BVISOR_LIB.

ERL_ROOT := $(shell erl -noshell -eval 'io:format("~s", [code:root_dir()])' -s init stop)
ERTS_INCLUDE := $(shell erl -noshell -eval 'io:format("~s/erts-~s/include", [code:root_dir(), erlang:system_info(version)])' -s init stop)

CC ?= cc
CFLAGS ?= -O2 -fPIC -Wall
LDFLAGS ?= -shared -ldl

PRIV := priv
EBIN := ebin

.PHONY: all clean

all: $(PRIV)/bvisor_nif.so $(EBIN)/bvisor.beam

$(PRIV)/bvisor_nif.so: c_src/bvisor_nif.c
	@mkdir -p $(PRIV)
	$(CC) $(CFLAGS) -I"$(ERTS_INCLUDE)" $(LDFLAGS) -o $@ $<

$(EBIN)/bvisor.beam: src/bvisor.erl
	@mkdir -p $(EBIN)
	erlc -o $(EBIN) $<

clean:
	rm -f $(PRIV)/bvisor_nif.so $(EBIN)/bvisor.beam
