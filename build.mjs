import { build } from "esbuild";
import ts from "typescript";

if (!process.argv.includes("--no-build")) {
  await build({
    entryPoints: ["src/index.ts"],
    bundle: true,
    format: "esm",
    outfile: "build/index.js",
    sourcemap: true,
    packages: "external",
    banner: { js: "#!/bin/env node" },
    minifyWhitespace: true,
    minifySyntax: true,
  });
}
if (!process.argv.includes("--no-check")) {
  const configPath = ts.findConfigFile(
    "./",
    ts.sys.fileExists,
    "tsconfig.json",
  );

  if (!configPath) {
    throw new Error("Could not find tsconfig.json");
  }

  const configFile = ts.readConfigFile(configPath, ts.sys.readFile);

  if (configFile.error) {
    throw new Error(
      ts.flattenDiagnosticMessageText(configFile.error.messageText, "\n"),
    );
  }

  const parsed = ts.parseJsonConfigFileContent(configFile.config, ts.sys, "./");

  const program = ts.createProgram({
    rootNames: parsed.fileNames,
    options: {
      ...parsed.options,
      declaration: true,
      emitDeclarationOnly: true,
    },
  });

  const emitResult = program.emit();

  const diagnostics = ts
    .getPreEmitDiagnostics(program)
    .concat(emitResult.diagnostics);

  if (diagnostics.length > 0) {
    console.error(
      ts.formatDiagnosticsWithColorAndContext(diagnostics, {
        getCanonicalFileName: (fileName) => fileName,
        getCurrentDirectory: process.cwd,
        getNewLine: () => "\n",
      }),
    );

    process.exit(1);
  }
}
console.log("Build complete.");
