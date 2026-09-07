import { z } from "zod";

export const TokenBaseSchema = z.object({
  line: z.number(),
  col: z.number(),
  raw: z.string(),
});

export const IdentifierSchema = TokenBaseSchema.extend({
  type: z.literal("Identifier"),
  name: z.string(),
});

export enum Keywords {
  LET = "let",
  FN = "fn",
  IMPORT = "import",
  BREAK = "break",
  IF = "if",
  FOR = "for",
  IN = "in",
  SWITCH = "switch",
  CASE = "case",
  DEFAULT = "default",
  STRUCT = "struct",
  ENUM = "enum",
}

export const KeywordSchema = TokenBaseSchema.extend({
  type: z.literal("Keyword"),
  keyword: z.enum(Keywords),
});

export enum Punctuation {
  NAMESPACE = "::",
  COLON = ":",
  SEMI = ";",
  RBRACE = "}",
  LBRACE = "{",
  RPAREN = ")",
  LPAREN = "(",
  RBRACKET = "]",
  LBRACKET = "[",
  AT = "@",
  STAR = "*",
  QUESTION = "?",
  POINT = "->",
  OR = "|",
  LOGICALOR = "||",
  XOR = "^",
  AND = "&",
  LOGICALAND = "&&",
  ASSIGN = "=",
  EQ = "==",
  NEQ = "!=",
  NOT = "!",
  COMMA = ",",
  DOT = ".",
  MOD = "%",
  DIV = "/",
  SUBNEG = "-",
}

export const PunctuationSchema = TokenBaseSchema.extend({
  type: z.literal("Punctuation"),
  symbol: z.enum(Punctuation),
});

export const IntegerLiteralSchema = TokenBaseSchema.extend({
  type: z.literal("IntegerLiteral"),
  base: z.literal([2, 8, 10, 16]),
  size: z.literal([8, 16, 32, 64]),
  value: z.bigint(),
});

export const FloatLiteralSchema = TokenBaseSchema.extend({
  type: z.literal("FloatLiteral"),
  value: z.float64(),
  size: z.literal([32, 64]),
});

export const StringLiteralSchema = TokenBaseSchema.extend({
  type: z.literal("StringLiteral"),
  value: z.string(),
});

export const TokenSchema = z.discriminatedUnion("type", [
  IdentifierSchema,
  KeywordSchema,
  IntegerLiteralSchema,
  PunctuationSchema,
  StringLiteralSchema,
  FloatLiteralSchema,
]);

export const TokenStreamSchema = z.array(TokenSchema);

export type FloatLiteral = z.infer<typeof FloatLiteralSchema>;
export type StringLiteral = z.infer<typeof StringLiteralSchema>;
export type Puncuation = z.infer<typeof PunctuationSchema>;
export type TokenBase = z.infer<typeof TokenBaseSchema>;
export type Identifier = z.infer<typeof IdentifierSchema>;
export type Keyword = z.infer<typeof KeywordSchema>;
export type Token = z.infer<typeof TokenSchema>;
export type TokenStream = z.infer<typeof TokenStreamSchema>;
