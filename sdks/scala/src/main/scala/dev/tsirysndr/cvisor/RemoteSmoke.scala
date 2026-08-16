package dev.tsirysndr.cvisor

import java.nio.charset.StandardCharsets

/** Remote-GraphQL e2e smoke for the Scala SDK's portable client
  * ([[RemoteSandbox]] over `java.net.http` — no libcvisor). Run against a
  * running cvisord:
  *
  * {{{
  * CVISOR_GRAPHQL_URL=http://127.0.0.1:8080/graphql CVISOR_TOKEN=... \
  *   sbt "runMain dev.tsirysndr.cvisor.RemoteSmoke"
  * }}}
  */
object RemoteSmoke:
  private def check(cond: Boolean, msg: => String): Unit =
    if !cond then
      System.err.println(s"assertion failed: $msg")
      sys.exit(1)

  def main(args: Array[String]): Unit =
    val url = sys.env
      .get("CVISOR_GRAPHQL_URL")
      .orElse(sys.env.get("CVISOR_URL"))
      .getOrElse("http://127.0.0.1:8080/graphql")
    val token = sys.env.getOrElse("CVISOR_TOKEN", "")
    val sb = RemoteSandbox(url, token)

    val health = sb.health()
    check(health.ok, s"health not ok: $health")

    val out = sb.run("echo hello")
    check(out.stdout == "hello\n", s"run stdout: $out")
    check(out.exitCode == 0, s"run exit code: $out")

    val info = sb.createSandbox()
    check(info.id.nonEmpty, s"createSandbox returned no id: $info")
    sb.writeFile("/tmp/data.txt", "round-trip\n")
    val data = new String(sb.readFile("/tmp/data.txt"), StandardCharsets.UTF_8)
    check(data == "round-trip\n", s"readFile round-trip: $data")
    sb.freeSandbox()

    println("SCALA_GRAPHQL_OK")
