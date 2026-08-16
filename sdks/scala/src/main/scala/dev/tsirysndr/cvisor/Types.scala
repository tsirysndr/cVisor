package dev.tsirysndr.cvisor

/** The captured result of a one-shot command run. */
final case class Output(stdout: String, stderr: String, exitCode: Int)

/** One guest environment variable. */
final case class EnvVar(key: String, value: String)

/** A sandbox's resource limits as reported by the daemon (nulls stay `None`). */
final case class SandboxLimit(
    memoryMax: Option[Long] = None,
    pidsMax: Option[Long] = None,
    cpuPercent: Option[Int] = None,
)

/** A sandbox's identity and configuration, mirroring the daemon's `Sandbox`. */
final case class SandboxInfo(
    id: String,
    name: String,
    allowNetwork: Boolean,
    allowListen: Boolean,
    env: Seq[EnvVar],
    limits: SandboxLimit,
)

/** One entry in a cache backend or the snapshot list. */
final case class CacheEntry(name: String, size: Long)

/** The daemon's liveness / build info. */
final case class Health(version: String, ok: Boolean)

/** cgroup v2 resource caps to send to the daemon; a `None` field is left unset. */
final case class Limits(
    memoryMax: Option[Long] = None,
    pidsMax: Option[Long] = None,
    cpuPercent: Option[Int] = None,
)

/** Optional per-run overrides for [[RemoteSandbox.run]]. */
final case class RunOptions(
    timeoutMs: Long = 0,
    allowNetwork: Option[Boolean] = None,
    limits: Option[Limits] = None,
    env: Map[String, String] = Map.empty,
)

/** The fields [[RemoteSandbox.configure]] may update; empty fields are unchanged. */
final case class ConfigureOptions(
    allowNetwork: Option[Boolean] = None,
    allowListen: Option[Boolean] = None,
    limits: Option[Limits] = None,
    env: Map[String, String] = Map.empty,
)
