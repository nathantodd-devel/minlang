export * from "./tokens";
export * from "./errors";
export * from "./utils";

import type { GenericError } from "../error";
import { Keywords, Punctuation, type TokenStream } from "./tokens";
import {
  InvalidNumericLiteralError,
  UnexpectedCharacterError,
  UnterminatedStringError,
} from "./errors";

type IntegerBase = 2 | 8 | 10 | 16;
type IntegerSize = 8 | 16 | 32 | 64;
type FloatSize = 32 | 64;

const INTEGER_SUFFIXES = new Map<string, IntegerSize>([
  ["u8", 8],
  ["u16", 16],
  ["u32", 32],
  ["u64", 64],
  ["s8", 8],
  ["s16", 16],
  ["s32", 32],
  ["s64", 64],
]);

const FLOAT_SUFFIXES = new Map<string, FloatSize>([
  ["f32", 32],
  ["f64", 64],
]);

type Tokens =
  { ok: true; tokens: TokenStream } | { ok: false; errors: GenericError[] };

export class Tokenizer {
  private tokens: TokenStream = [];
  private errors: GenericError[] = [];

  constructor(
    private contents: string,
    private file: string = "stdin",
  ) {}

  tokenize(): Tokens {
    this.tokens = [];
    this.errors = [];

    const lines = this.contents.split(/\r?\n/);

    for (let line = 0; line < lines.length; line++) {
      const source = lines[line] ?? "";
      let col = 0;

      while (col < source.length) {
        const char = source[col];

        if (char === undefined) {
          break;
        }

        if (/\s/.test(char)) {
          col++;
          continue;
        }

        if (char === "/" && source[col + 1] === "/") {
          break;
        }

        if (/[a-zA-Z_]/.test(char)) {
          const start = col;

          col++;

          while (col < source.length) {
            const current = source[col];

            if (current === undefined || !/[a-zA-Z0-9_]/.test(current)) {
              break;
            }

            col++;
          }

          const raw = source.slice(start, col);

          const keyword = Object.values(Keywords).find(
            (value) => value === raw,
          );

          if (keyword !== undefined) {
            this.tokens.push({
              type: "Keyword",
              keyword,
              line,
              col: start,
              raw,
            });
          } else {
            this.tokens.push({
              type: "Identifier",
              name: raw,
              line,
              col: start,
              raw,
            });
          }

          continue;
        }

        if (/[0-9]/.test(char)) {
          col = this.tokenizeNumber(source, line, col);
          continue;
        }

        if (char === '"' || char === "'") {
          col = this.tokenizeString(source, line, col);
          continue;
        }

        const two = source.slice(col, col + 2);

        const twoCharacterPunctuation = [
          Punctuation.NAMESPACE,
          Punctuation.POINT,
          Punctuation.EQ,
          Punctuation.NEQ,
        ] as const;

        if (
          twoCharacterPunctuation.includes(
            two as (typeof twoCharacterPunctuation)[number],
          )
        ) {
          this.tokens.push({
            type: "Punctuation",
            symbol: two as (typeof twoCharacterPunctuation)[number],
            line,
            col,
            raw: two,
          });

          col += 2;
          continue;
        }

        const punctuation = Object.values(Punctuation).find(
          (value) => value.length === 1 && value === char,
        );

        if (punctuation !== undefined) {
          this.tokens.push({
            type: "Punctuation",
            symbol: punctuation,
            line,
            col,
            raw: char,
          });

          col++;
          continue;
        }

        this.errors.push(
          new UnexpectedCharacterError(this.file, line, col, char),
        );

        col++;
      }
    }

    if (this.errors.length !== 0) {
      return { ok: false, errors: this.errors };
    }

    return { ok: true, tokens: this.tokens };
  }

  private tokenizeNumber(source: string, line: number, start: number): number {
    let col = start;

    const first = source[col];

    if (first === undefined) {
      return col;
    }

    if (first === "0") {
      const prefix = source[col + 1]?.toLowerCase();

      if (prefix === "x") {
        return this.tokenizePrefixedInteger(
          source,
          line,
          start,
          16,
          /[0-9a-fA-F]/,
          "0x",
        );
      }

      if (prefix === "b") {
        return this.tokenizePrefixedInteger(
          source,
          line,
          start,
          2,
          /[01]/,
          "0b",
        );
      }

      if (prefix === "o") {
        return this.tokenizePrefixedInteger(
          source,
          line,
          start,
          8,
          /[0-7]/,
          "0o",
        );
      }
    }

    while (col < source.length && /[0-9]/.test(source[col] ?? "")) {
      col++;
    }

    let isFloat = false;

    if (source[col] === ".") {
      isFloat = true;
      col++;

      while (col < source.length && /[0-9]/.test(source[col] ?? "")) {
        col++;
      }
    }

    if (source[col]?.toLowerCase() === "e") {
      isFloat = true;

      col++;

      if (source[col] === "+" || source[col] === "-") {
        col++;
      }

      const exponentStart = col;

      while (col < source.length && /[0-9]/.test(source[col] ?? "")) {
        col++;
      }

      if (col === exponentStart) {
        this.errors.push(
          new InvalidNumericLiteralError(
            `Invalid exponent in numeric literal`,
            line,
            exponentStart,
            this.file,
          ),
        );

        return col;
      }
    }

    const suffixStart = col;

    while (col < source.length && /[a-zA-Z0-9]/.test(source[col] ?? "")) {
      col++;
    }

    const suffix = source.slice(suffixStart, col);

    if (isFloat) {
      const size = suffix ? FLOAT_SUFFIXES.get(suffix) : 64;

      if (suffix && size === undefined) {
        // Don't silently turn something like `2.5foo` into
        // FloatLiteral + Identifier.
        return col;
      }

      const raw = source.slice(start, col);
      const numericPart = suffix ? raw.slice(0, -suffix.length) : raw;

      const value = Number(numericPart);

      this.tokens.push({
        type: "FloatLiteral",
        value,
        size: size ?? 64,
        line,
        col: start,
        raw,
      });

      return col;
    }

    const size = suffix ? INTEGER_SUFFIXES.get(suffix) : 32;

    if (suffix && size === undefined) {
      // Unknown suffix.
      return col;
    }

    const raw = source.slice(start, col);
    const numericPart = suffix ? raw.slice(0, -suffix.length) : raw;

    // Legacy leading-zero octal.
    //
    // 0673 -> base 8
    //
    // Keep plain `0` decimal.
    let base: IntegerBase = 10;

    if (numericPart.length > 1 && numericPart.startsWith("0")) {
      base = 8;
    }

    const value = BigInt(
      base === 8 ? this.parseBigInt(numericPart, 8) : numericPart,
    );

    this.tokens.push({
      type: "IntegerLiteral",
      base,
      size: size ?? 32,
      value,
      line,
      col: start,
      raw,
    });

    return col;
  }

  private tokenizePrefixedInteger(
    source: string,
    line: number,
    start: number,
    base: IntegerBase,
    digitPattern: RegExp,
    prefix: string,
  ): number {
    let col = start + prefix.length;

    const digitsStart = col;

    while (col < source.length && digitPattern.test(source[col] ?? "")) {
      col++;
    }

    // 0x, 0b, or 0o with no digits.
    if (col === digitsStart) {
      this.errors.push(
        new InvalidNumericLiteralError(
          `Invalid ${prefix} numeric literal`,
          line,
          start,
          this.file,
        ),
      );

      return col;
    }

    const suffixStart = col;

    while (col < source.length && /[a-zA-Z0-9]/.test(source[col] ?? "")) {
      col++;
    }

    const suffix = source.slice(suffixStart, col);

    const size = suffix ? INTEGER_SUFFIXES.get(suffix) : 32;

    if (suffix && size === undefined) {
      this.errors.push(
        new InvalidNumericLiteralError(
          `Invalid integer suffix '${suffix}'`,
          line,
          suffixStart,
          this.file,
        ),
      );

      return col;
    }

    const raw = source.slice(start, col);

    const numericPart = suffix ? raw.slice(0, -suffix.length) : raw;

    const digits = numericPart.slice(prefix.length);

    let value: bigint;

    try {
      value = BigInt(
        base === 16
          ? `0x${digits}`
          : base === 2
            ? `0b${digits}`
            : `0o${digits}`,
      );
    } catch {
      this.errors.push(
        new InvalidNumericLiteralError(
          `Invalid numeric literal '${raw}'`,
          line,
          start,
          this.file,
        ),
      );

      return col;
    }

    this.tokens.push({
      type: "IntegerLiteral",
      base,
      size: size ?? 32,
      value,
      line,
      col: start,
      raw,
    });

    return col;
  }

  private parseBigInt(value: string, base: IntegerBase): string {
    if (base === 8) {
      return `0o${value.slice(1)}`;
    }

    return value;
  }

  private tokenizeString(source: string, line: number, start: number): number {
    let col = start + 1;
    let value = "";

    while (col < source.length) {
      const current = source[col];

      if (current === undefined) {
        break;
      }

      if (current === '"' || current === "'") {
        const raw = source.slice(start, col + 1);

        this.tokens.push({
          type: "StringLiteral",
          value,
          line,
          col: start,
          raw,
        });

        return col + 1;
      }

      if (current === "\\") {
        const next = source[col + 1];

        if (next === undefined) {
          this.errors.push(new UnterminatedStringError(line, start, this.file));

          return source.length;
        }

        switch (next) {
          case "n":
            value += "\n";
            break;

          case "r":
            value += "\r";
            break;

          case "t":
            value += "\t";
            break;

          case "\\":
            value += "\\";
            break;

          case '"':
            value += '"';
            break;

          case "0":
            value += "\0";
            break;

          default:
            value += next;
            break;
        }

        col += 2;
        continue;
      }

      value += current;
      col++;
    }

    this.errors.push(new UnterminatedStringError(line, start, this.file));

    return source.length;
  }
}
