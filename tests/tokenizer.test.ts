import { expect, test } from "vitest";
import * as tkn from "../src/tokenizer";

test("Invalid float bit width", () => {
  let tok = new tkn.Tokenizer("100.f22");
  const res = tok.tokenize();
  expect(res.ok).toBe(false);
});

test("Invalid int bit width", () => {
  let tok = new tkn.Tokenizer("100u22");
  const res = tok.tokenize();
  expect(res.ok).toBe(false);
});

test("Unexpected Character", () => {
  let tok = new tkn.Tokenizer("let unexpected_dollar = $;");
  const res = tok.tokenize();
  expect(res.ok).toBe(false);
});

test("Unterminated String", () => {
  let tok = new tkn.Tokenizer('let unterminated_string_1 = "hello;');
  const res = tok.tokenize();
  expect(res.ok).toBe(false);
});

test("Werid Float Syntax", () => {
  let tok = new tkn.Tokenizer("let float6 = 2f32;");
  const res = tok.tokenize();
  expect(res.ok).toBe(true);
  if (!res.ok) return;
  expect(res.tokens[3].type).toBe("FloatLiteral");
});

// -----------------------------------------------------------------------------
// Number parsing
// -----------------------------------------------------------------------------

test("Decimal integer", () => {
  const tok = new tkn.Tokenizer("123");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens).toHaveLength(1);
  expect(res.tokens[0]).toMatchObject({
    type: "IntegerLiteral",
    value: 123n,
    base: 10,
    size: 32,
    signed: true,
    raw: "123",
  });
});

test("Zero is decimal", () => {
  const tok = new tkn.Tokenizer("0");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "IntegerLiteral",
    value: 0n,
    base: 10,
    size: 32,
    signed: true,
  });
});

test("Unsigned integer suffixes", () => {
  const tok = new tkn.Tokenizer("1u8 2u16 3u32 4u64");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens).toHaveLength(4);

  expect(res.tokens[0]).toMatchObject({
    type: "IntegerLiteral",
    value: 1n,
    size: 8,
    signed: false,
  });

  expect(res.tokens[1]).toMatchObject({
    type: "IntegerLiteral",
    value: 2n,
    size: 16,
    signed: false,
  });

  expect(res.tokens[2]).toMatchObject({
    type: "IntegerLiteral",
    value: 3n,
    size: 32,
    signed: false,
  });

  expect(res.tokens[3]).toMatchObject({
    type: "IntegerLiteral",
    value: 4n,
    size: 64,
    signed: false,
  });
});

test("Signed integer suffixes", () => {
  const tok = new tkn.Tokenizer("1s8 2s16 3s32 4s64");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens).toHaveLength(4);

  expect(res.tokens[0]).toMatchObject({
    type: "IntegerLiteral",
    value: 1n,
    size: 8,
    signed: true,
  });

  expect(res.tokens[1]).toMatchObject({
    type: "IntegerLiteral",
    value: 2n,
    size: 16,
    signed: true,
  });

  expect(res.tokens[2]).toMatchObject({
    type: "IntegerLiteral",
    value: 3n,
    size: 32,
    signed: true,
  });

  expect(res.tokens[3]).toMatchObject({
    type: "IntegerLiteral",
    value: 4n,
    size: 64,
    signed: true,
  });
});

test("Hexadecimal integer", () => {
  const tok = new tkn.Tokenizer("0xff");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "IntegerLiteral",
    value: 255n,
    base: 16,
    size: 32,
    signed: true,
    raw: "0xff",
  });
});

test("Hexadecimal unsigned integer", () => {
  const tok = new tkn.Tokenizer("0xFFu64");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "IntegerLiteral",
    value: 255n,
    base: 16,
    size: 64,
    signed: false,
    raw: "0xFFu64",
  });
});

test("Binary integer", () => {
  const tok = new tkn.Tokenizer("0b101010");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "IntegerLiteral",
    value: 42n,
    base: 2,
    size: 32,
    signed: true,
    raw: "0b101010",
  });
});

test("Octal integer", () => {
  const tok = new tkn.Tokenizer("0o755");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "IntegerLiteral",
    value: 493n,
    base: 8,
    size: 32,
    signed: true,
    raw: "0o755",
  });
});

test("Legacy leading-zero octal", () => {
  const tok = new tkn.Tokenizer("0673");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "IntegerLiteral",
    value: 443n,
    base: 8,
    size: 32,
    signed: true,
    raw: "0673",
  });
});

test("Invalid legacy octal", () => {
  const tok = new tkn.Tokenizer("089");
  const res = tok.tokenize();

  expect(res.ok).toBe(false);
});

test("Missing hexadecimal digits", () => {
  const tok = new tkn.Tokenizer("0x");
  const res = tok.tokenize();

  expect(res.ok).toBe(false);
});

test("Missing binary digits", () => {
  const tok = new tkn.Tokenizer("0b");
  const res = tok.tokenize();

  expect(res.ok).toBe(false);
});

test("Missing octal digits", () => {
  const tok = new tkn.Tokenizer("0o");
  const res = tok.tokenize();

  expect(res.ok).toBe(false);
});

test("Float", () => {
  const tok = new tkn.Tokenizer("12.5");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "FloatLiteral",
    value: 12.5,
    size: 64,
    raw: "12.5",
  });
});

test("Float f32 suffix", () => {
  const tok = new tkn.Tokenizer("12.5f32");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "FloatLiteral",
    value: 12.5,
    size: 32,
    raw: "12.5f32",
  });
});

test("Float f64 suffix", () => {
  const tok = new tkn.Tokenizer("12.5f64");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "FloatLiteral",
    value: 12.5,
    size: 64,
    raw: "12.5f64",
  });
});

test("Integer-looking f32 is a float", () => {
  const tok = new tkn.Tokenizer("2f32");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "FloatLiteral",
    value: 2,
    size: 32,
    raw: "2f32",
  });
});

test("Integer-looking f64 is a float", () => {
  const tok = new tkn.Tokenizer("2f64");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "FloatLiteral",
    value: 2,
    size: 64,
    raw: "2f64",
  });
});

test("Scientific notation", () => {
  const tok = new tkn.Tokenizer("1e10");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "FloatLiteral",
    value: 1e10,
    size: 64,
    raw: "1e10",
  });
});

test("Scientific notation with positive exponent", () => {
  const tok = new tkn.Tokenizer("1e+10");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "FloatLiteral",
    value: 1e10,
    size: 64,
  });
});

test("Scientific notation with negative exponent", () => {
  const tok = new tkn.Tokenizer("1e-3");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "FloatLiteral",
    value: 0.001,
    size: 64,
  });
});

test("Scientific notation with f32", () => {
  const tok = new tkn.Tokenizer("1.5e2f32");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "FloatLiteral",
    value: 150,
    size: 32,
    raw: "1.5e2f32",
  });
});

test("Invalid exponent", () => {
  const tok = new tkn.Tokenizer("1e");
  const res = tok.tokenize();

  expect(res.ok).toBe(false);
});

test("Invalid exponent with sign", () => {
  const tok = new tkn.Tokenizer("1e+");
  const res = tok.tokenize();

  expect(res.ok).toBe(false);
});

test("Invalid float suffix", () => {
  const tok = new tkn.Tokenizer("12.5foo");
  const res = tok.tokenize();

  expect(res.ok).toBe(false);
});

test("Invalid integer suffix", () => {
  const tok = new tkn.Tokenizer("123foo");
  const res = tok.tokenize();

  expect(res.ok).toBe(false);
});

test("Prefixed integer with invalid suffix", () => {
  const tok = new tkn.Tokenizer("0xfffoo");
  const res = tok.tokenize();

  expect(res.ok).toBe(false);
});

test("Multiple numbers", () => {
  const tok = new tkn.Tokenizer("1 2u8 3.5f32 0xff");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens).toHaveLength(4);

  expect(res.tokens[0].type).toBe("IntegerLiteral");
  expect(res.tokens[1].type).toBe("IntegerLiteral");
  expect(res.tokens[2].type).toBe("FloatLiteral");
  expect(res.tokens[3].type).toBe("IntegerLiteral");
});

test("Numbers adjacent to punctuation", () => {
  const tok = new tkn.Tokenizer("foo(123, 4.5);");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[2]).toMatchObject({
    type: "IntegerLiteral",
    value: 123n,
  });

  expect(res.tokens[4]).toMatchObject({
    type: "FloatLiteral",
    value: 4.5,
  });
});

test("Integer-looking f32 is a float", () => {
  const tok = new tkn.Tokenizer("2f32");
  const res = tok.tokenize();

  expect(res.ok).toBe(true);
  if (!res.ok) return;

  expect(res.tokens[0]).toMatchObject({
    type: "FloatLiteral",
    value: 2,
    size: 32,
    raw: "2f32",
  });
});
