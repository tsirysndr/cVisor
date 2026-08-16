ThisBuild / scalaVersion := "3.3.6"
ThisBuild / organization := "dev.tsirysndr"
ThisBuild / version      := "0.1.0"

lazy val root = (project in file("."))
  .settings(
    name := "cvisor",
    libraryDependencies += "com.lihaoyi" %% "upickle" % "4.1.0",
    scalacOptions ++= Seq("-deprecation", "-feature", "-release", "22"),
    // The native FFI path (java.lang.foreign) needs a JDK 22+ toolchain and the
    // restricted-method access flag at run time.
    run / fork := true,
    Test / fork := true,
    run / javaOptions += "--enable-native-access=ALL-UNNAMED",
    Test / javaOptions += "--enable-native-access=ALL-UNNAMED",
  )
