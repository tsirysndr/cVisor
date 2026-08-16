package dev.tsirysndr.cvisor

/** A tiny smoke test for the portable GraphQL path. Point it at a running
  * daemon:
  *
  * {{{
  * CVISOR_URL=http://127.0.0.1:8080/graphql CVISOR_TOKEN=... \
  *   sbt "runMain dev.tsirysndr.cvisor.Smoke"
  * }}}
  */
object Smoke:
  def main(args: Array[String]): Unit =
    val url = sys.env.getOrElse("CVISOR_URL", "http://127.0.0.1:8080/graphql")
    val token = sys.env.getOrElse("CVISOR_TOKEN", "")
    val sb = RemoteSandbox(url, token)

    val health = sb.health()
    println(s"daemon ${health.version} ok=${health.ok}")

    val out = sb.run("echo hi")
    print(out.stdout) // "hi\n"
    println(s"exit ${out.exitCode}")
