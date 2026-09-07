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
