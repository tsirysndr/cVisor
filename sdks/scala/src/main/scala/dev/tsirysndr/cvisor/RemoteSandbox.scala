package dev.tsirysndr.cvisor

import java.util.Base64

/** A high-level, typed client for the daemon that mirrors the GraphQL
  * operations. It optionally binds to a sandbox id (an empty id means an
  * ephemeral sandbox for [[run]]). This is the primary, portable API and works
  * on any OS.
  */
final class RemoteSandbox(val client: GraphQLClient, initialId: String = ""):
  import RemoteSandbox.*

  private var sandboxId: String = initialId

  /** The bound sandbox id ("" if unbound). */
  def id: String = sandboxId

  /** A copy of this handle bound to a different sandbox id, sharing the client. */
  def withId(newId: String): RemoteSandbox = new RemoteSandbox(client, newId)

  /** Probe the daemon. */
  def health(): Health =
    val h = client.query("{ health { version ok } }")("health")
    Health(h("version").str, h("ok").bool)

  /** Create a sandbox (empty name gets a random one) and bind this handle to it. */
  def createSandbox(name: String = ""): SandboxInfo =
    val doc = s"mutation($$name:String){ createSandbox(name:$$name){ $Fields } }"
    val vars = ujson.Obj()
    if name.nonEmpty then vars("name") = name
    val info = parseSandbox(client.mutate(doc, vars)("createSandbox"))
    sandboxId = info.id
    info

  /** List sandboxes, optionally substring-filtered and paginated (`limit` &lt; 0
    * returns all).
    */
  def listSandboxes(search: String = "", limit: Int = -1, offset: Int = 0): Seq[SandboxInfo] =
    val doc =
      s"query($$search:String,$$limit:Int,$$offset:Int){ sandboxes(search:$$search,limit:$$limit,offset:$$offset){ $Fields } }"
    val vars = ujson.Obj()
    if search.nonEmpty then vars("search") = search
    if limit >= 0 then vars("limit") = limit
    if offset > 0 then vars("offset") = offset
    client.query(doc, vars)("sandboxes").arr.map(parseSandbox).toSeq

  /** Free the bound sandbox and clean up its overlay. */
  def freeSandbox(): Unit =
    client.mutate("mutation($id:String!){ freeSandbox(id:$id) }", ujson.Obj("id" -> sandboxId))

  /** Update the bound sandbox's network/listen flags, limits, and env. */
  def configure(opts: ConfigureOptions): SandboxInfo =
    val doc =
      s"mutation($$id:String!,$$allowNetwork:Boolean,$$allowListen:Boolean,$$limits:LimitsInput,$$env:[EnvVarInput!]){ configure(id:$$id,allowNetwork:$$allowNetwork,allowListen:$$allowListen,limits:$$limits,env:$$env){ $Fields } }"
    val vars = ujson.Obj("id" -> sandboxId)
    opts.allowNetwork.foreach(v => vars("allowNetwork") = v)
    opts.allowListen.foreach(v => vars("allowListen") = v)
    opts.limits.foreach(l => vars("limits") = limitsVar(l))
    if opts.env.nonEmpty then vars("env") = envVar(opts.env)
    parseSandbox(client.mutate(doc, vars)("configure"))

  /** Run `command` in the bound sandbox (or ephemerally when unbound). */
  def run(command: String, opts: RunOptions = RunOptions()): Output =
    val doc =
      "mutation($id:String,$command:String!,$timeoutMs:Int,$allowNetwork:Boolean,$limits:LimitsInput,$env:[EnvVarInput!]){ run(id:$id,command:$command,timeoutMs:$timeoutMs,allowNetwork:$allowNetwork,limits:$limits,env:$env){ stdout stderr exitCode } }"
    val vars = ujson.Obj("command" -> command)
    if sandboxId.nonEmpty then vars("id") = sandboxId
    if opts.timeoutMs > 0 then vars("timeoutMs") = num(opts.timeoutMs)
    opts.allowNetwork.foreach(v => vars("allowNetwork") = v)
    opts.limits.foreach(l => vars("limits") = limitsVar(l))
    if opts.env.nonEmpty then vars("env") = envVar(opts.env)
    val r = client.mutate(doc, vars)("run")
    Output(r("stdout").str, r("stderr").str, r("exitCode").num.toInt)

  /** Run `command`, SIGKILLing the guest after `timeoutMs` (exit code 137). */
  def run(command: String, timeoutMs: Long): Output =
    run(command, RunOptions(timeoutMs = timeoutMs))

  /** Write `data` to `path` inside the bound sandbox's overlay. */
  def writeFile(path: String, data: Array[Byte]): Unit =
    val doc = "mutation($id:String!,$path:String!,$data:String!){ writeFile(id:$id,path:$path,dataBase64:$data) }"
    val vars = ujson.Obj("id" -> sandboxId, "path" -> path, "data" -> Base64.getEncoder.encodeToString(data))
    client.mutate(doc, vars)

  /** Write a UTF-8 string to `path`. */
  def writeFile(path: String, data: String): Unit =
    writeFile(path, data.getBytes(java.nio.charset.StandardCharsets.UTF_8))

  /** Read `path` from the bound sandbox, decoding the base64 payload. */
  def readFile(path: String): Array[Byte] =
    val doc = "query($id:String!,$path:String!){ readFile(id:$id,path:$path) }"
    val s = client.query(doc, ujson.Obj("id" -> sandboxId, "path" -> path))("readFile").str
    Base64.getDecoder.decode(s)

  /** Snapshot the bound sandbox's overlay (empty id gets a generated one). */
  def snapshot(snapshotId: String = ""): String =
    val doc = "mutation($id:String!,$snapshotId:String){ snapshot(id:$id,snapshotId:$snapshotId) }"
    val vars = ujson.Obj("id" -> sandboxId)
    if snapshotId.nonEmpty then vars("snapshotId") = snapshotId
    client.mutate(doc, vars)("snapshot").str

  /** Replace the bound sandbox's overlay with a snapshot (discard later changes). */
  def rollback(snapshotId: String): Unit =
    val doc = "mutation($id:String!,$snapshotId:String!){ rollback(id:$id,snapshotId:$snapshotId) }"
    client.mutate(doc, ujson.Obj("id" -> sandboxId, "snapshotId" -> snapshotId))

  /** Create a new sandbox from a snapshot. */
  def branch(snapshotId: String, name: String = ""): SandboxInfo =
    val doc = s"mutation($$snapshotId:String!,$$name:String){ branch(snapshotId:$$snapshotId,name:$$name){ $Fields } }"
    val vars = ujson.Obj("snapshotId" -> snapshotId)
    if name.nonEmpty then vars("name") = name
    parseSandbox(client.mutate(doc, vars)("branch"))

  /** Fork the bound sandbox's current overlay into a new sandbox. */
  def fork(name: String = ""): SandboxInfo =
    val doc = s"mutation($$id:String!,$$name:String){ fork(id:$$id,name:$$name){ $Fields } }"
    val vars = ujson.Obj("id" -> sandboxId)
    if name.nonEmpty then vars("name") = name
    parseSandbox(client.mutate(doc, vars)("fork"))

  /** List saved overlay snapshots. */
  def snapshots(): Seq[CacheEntry] =
    client.query("{ snapshots { name size } }")("snapshots").arr.map(parseCacheEntry).toSeq

  /** Delete a snapshot; returns whether it existed. */
  def deleteSnapshot(snapshotId: String): Boolean =
    client.mutate("mutation($id:String!){ deleteSnapshot(id:$id) }", ujson.Obj("id" -> snapshotId))(
      "deleteSnapshot",
    ).bool

  /** Archive a sandbox path under `key` (empty backend/format use the daemon
    * defaults: disk backend, gzip).
    */
  def cacheSave(path: String, key: String, backend: String = "", format: String = ""): Unit =
    cacheOp("cacheSave", path, key, backend, format)

  /** Restore a cached entry into a sandbox path. */
  def cacheRestore(path: String, key: String, backend: String = "", format: String = ""): Unit =
    cacheOp("cacheRestore", path, key, backend, format)

  private def cacheOp(op: String, path: String, key: String, backend: String, format: String): Unit =
    val doc =
      s"mutation($$id:String!,$$path:String!,$$key:String!,$$backend:String,$$format:String){ $op(id:$$id,path:$$path,key:$$key,backend:$$backend,format:$$format) }"
    val vars = ujson.Obj("id" -> sandboxId, "path" -> path, "key" -> key)
    if backend.nonEmpty then vars("backend") = backend
    if format.nonEmpty then vars("format") = format
    client.mutate(doc, vars)

  /** List entries in a cache backend (empty backend = default). */
  def cacheList(backend: String = ""): Seq[CacheEntry] =
    val doc = "query($backend:String){ cacheList(backend:$backend){ name size } }"
    val vars = ujson.Obj()
    if backend.nonEmpty then vars("backend") = backend
    client.query(doc, vars)("cacheList").arr.map(parseCacheEntry).toSeq

object RemoteSandbox:
  /** Build a handle talking to the daemon at `url` with a fresh client. */
  def apply(url: String, token: String): RemoteSandbox =
    new RemoteSandbox(new GraphQLClient(url, token))

  private val Fields =
    "id name allowNetwork allowListen env{key value} limits{memoryMax pidsMax cpuPercent}"

  private def num(v: Long): ujson.Value = ujson.Num(v.toDouble)

  private def optLong(v: ujson.Value): Option[Long] = v match
    case ujson.Null => None
    case n          => Some(n.num.toLong)

  private def optInt(v: ujson.Value): Option[Int] = v match
    case ujson.Null => None
    case n          => Some(n.num.toInt)

  private def parseSandbox(v: ujson.Value): SandboxInfo =
    SandboxInfo(
      id = v("id").str,
      name = v("name").str,
      allowNetwork = v("allowNetwork").bool,
      allowListen = v("allowListen").bool,
      env = v("env").arr.map(e => EnvVar(e("key").str, e("value").str)).toSeq,
      limits = SandboxLimit(optLong(v("limits")("memoryMax")), optLong(v("limits")("pidsMax")), optInt(v("limits")("cpuPercent"))),
    )

  private def parseCacheEntry(v: ujson.Value): CacheEntry =
    CacheEntry(v("name").str, v("size").num.toLong)

  private def limitsVar(l: Limits): ujson.Value =
    val o = ujson.Obj()
    l.memoryMax.foreach(v => o("memoryMax") = num(v))
    l.pidsMax.foreach(v => o("pidsMax") = num(v))
    l.cpuPercent.foreach(v => o("cpuPercent") = v)
    o

  private def envVar(env: Map[String, String]): ujson.Value =
    ujson.Arr.from(env.map((k, v) => ujson.Obj("key" -> k, "value" -> v)))
