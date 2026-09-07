import { readFile } from "fs/promises";
import { Tokenizer, TokenStreamSchema } from "./tokenizer";

export * from "./parser";
export * from "./error";
export * from "./tokenizer";

function printUsage() {
  console.error(`Usage: MEcc [options] <file>`);
}

export async function main(): Promise<number> {
  const [argc, argv, envp] = [
    process.argv.length - 1,
    process.argv.slice(1),
    process.env,
  ];
  if (argc < 2) {
    printUsage();
    return 1;
  }
  const contents = await readFile(argv[1]!, {
    encoding: "utf-8",
  });

  const lexer = new Tokenizer(contents, argv[1]!);
  const start = process.hrtime.bigint();
  const res = lexer.tokenize();
  const end = process.hrtime.bigint();

  const elapsedNs = end - start;
  const elapsedMs = Number(elapsedNs) / 1_000_000;

  if (!res.ok) {
    res.errors.forEach((e) => {
      console.error(e.format());
    });
    return 1;
  }
  TokenStreamSchema.parse(res.tokens);

  console.log(
    JSON.stringify(res.tokens, (_, v) =>
      typeof v === "bigint" ? v.toString() : v,
    ),
  );
  console.log(`Tokenization: ${elapsedMs.toFixed(3)} ms`);

  return 0;
}
