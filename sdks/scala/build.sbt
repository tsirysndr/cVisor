// The groupId is `io.github.tsirysndr` (matching the Clojure SDK): an
// `io.github.<user>` namespace is verified by proving ownership of that GitHub
// account, far less setup than the DNS record a custom domain needs. The Scala
// package stays `dev.tsirysndr.cvisor` — only the Maven coordinates change.
ThisBuild / organization  := "io.github.tsirysndr"
ThisBuild / version       := "0.1.0"
ThisBuild / scalaVersion  := "3.3.6"
// Declared so downstream tooling can reason about eviction and sbt stops
// warning on publish; `early-semver` is right for 0.x (a minor may break).
ThisBuild / versionScheme := Some("early-semver")

lazy val root = (project in file("."))
  .settings(
    name := "cvisor",
    description := "cVisor SDK — a portable GraphQL client for the daemon (any OS) " +
      "plus a Linux-only in-process native sandbox (Java FFM over libcvisor).",
    homepage := Some(url("https://github.com/tsirysndr/cVisor")),
    licenses := List("MIT" -> url("https://opensource.org/license/mit")),
    libraryDependencies += "com.lihaoyi" %% "upickle" % "4.1.0",
    scalacOptions ++= Seq("-deprecation", "-feature", "-release", "22"),
    // The native FFI path (java.lang.foreign) needs a JDK 22+ toolchain and the
    // restricted-method access flag at run time.
    run / fork := true,
    Test / fork := true,
    run / javaOptions += "--enable-native-access=ALL-UNNAMED",
    Test / javaOptions += "--enable-native-access=ALL-UNNAMED",

    // -- publishing ---------------------------------------------------------
    //
    // Maven Central, through Sonatype's Central Portal. Two steps:
    //
    //   sbt publishSigned      # stage a signed bundle under target/sonatype-staging
    //   ./publish-central.sh   # zip it and POST it to the Portal's publisher API
    //
    // `sonatypeBundleRelease` is NOT used: sbt-sonatype (3.12.2, the newest)
    // speaks the legacy Nexus staging API, which the Portal doesn't serve — its
    // compatibility host answers only part of it and the release path 400s. The
    // Portal's own API takes the whole staged bundle as one zip, which is what
    // publish-central.sh uploads. See that script's header for the details.
    //
    // Sign with a specific *published* key, not gpg's default: Central verifies
    // the signature against a public keyserver and rejects a key it can't find.
    // This one is on keys.openpgp.org and matches the `developers` entry.
    pgpSigningKey := Some("CE4443A333319648"),
    // publishSigned stages the signed bundle locally; the credential host only
    // needs to be the OSSRH compatibility host (not "central.sonatype.com",
    // which 404s the staging paths sbt-sonatype uses).
    publishTo := sonatypePublishToBundle.value,
    sonatypeCredentialHost := "ossrh-staging-api.central.sonatype.com",
    publishMavenStyle := true,
    // Central rejects a POM without these.
    scmInfo := Some(
      ScmInfo(
        url("https://github.com/tsirysndr/cVisor"),
        "scm:git:https://github.com/tsirysndr/cVisor.git"
      )
    ),
    developers := List(
      Developer(
        id = "tsirysndr",
        name = "Tsiry Sandratraina",
        email = "tsiry.sndr@rocksky.app",
        url = url("https://github.com/tsirysndr")
      )
    )
  )
