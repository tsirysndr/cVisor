package dev.tsirysndr.cvisor

import java.net.URI
import java.net.http.{HttpClient, HttpRequest, HttpResponse}
import java.nio.charset.StandardCharsets
import java.time.Duration

/** Raised when the daemon replies with a non-empty `errors` array (or a
  * non-2xx status with no parseable GraphQL body).
  */
final class GraphQLError(val messages: Seq[String])
    extends RuntimeException(
      if messages.isEmpty then "graphql error" else messages.mkString("; "),
    )

/** A thin, portable client for the cVisor daemon's GraphQL HTTP endpoint.
  *
  * Uses only the JDK's `java.net.http.HttpClient` plus uPickle/ujson, so it
  * works on any OS (macOS included) and never loads the native `libcvisor`.
  */
final class GraphQLClient(
    url: String,
    token: String,
    http: HttpClient = GraphQLClient.defaultHttp,
):

  /** Run a GraphQL query document; returns the `data` object. */
  def query(document: String, variables: ujson.Value = ujson.Obj()): ujson.Value =
    execute(document, variables)

  /** Run a GraphQL mutation document; returns the `data` object. */
  def mutate(document: String, variables: ujson.Value = ujson.Obj()): ujson.Value =
    execute(document, variables)

  private def execute(document: String, variables: ujson.Value): ujson.Value =
    val payload = ujson.Obj("query" -> document, "variables" -> variables)
    val request = HttpRequest
      .newBuilder()
      .uri(URI.create(url))
      .timeout(Duration.ofSeconds(120))
      .header("Content-Type", "application/json")
      .header("Authorization", s"Bearer $token")
      .POST(HttpRequest.BodyPublishers.ofString(ujson.write(payload), StandardCharsets.UTF_8))
      .build()

    val response = http.send(request, HttpResponse.BodyHandlers.ofString())
    val status = response.statusCode()

    val parsed =
      try ujson.read(response.body())
      catch
        case _: Throwable =>
          if status < 200 || status >= 300 then
            throw new GraphQLError(Seq(s"HTTP $status from daemon"))
          else throw new GraphQLError(Seq("invalid JSON response from daemon"))

    parsed.obj.get("errors").map(_.arr).filter(_.nonEmpty).foreach { errs =>
      val msgs = errs.map(e => e.obj.get("message").map(_.str).getOrElse(ujson.write(e))).toSeq
      throw new GraphQLError(msgs)
    }

    if status < 200 || status >= 300 then
      throw new GraphQLError(Seq(s"HTTP $status from daemon"))

    parsed.obj.getOrElse("data", ujson.Null)

object GraphQLClient:
  /** A shared, lazily built `HttpClient` used when a caller supplies none. */
  lazy val defaultHttp: HttpClient =
    HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(30)).build()
