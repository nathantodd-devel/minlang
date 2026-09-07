import { TokenStreamSchema } from "../tokenizer";
import { z } from "zod";

export const LocationSchema = z.object({
  start: z.object({
    line: z.number(),
    col: z.number(),
  }),
  end: z.object({
    line: z.number(),
    col: z.number(),
  }),
});

export const ExpressionBaseSchema = z.object({
  position: LocationSchema,
  raw: z.string(),
  tokens: TokenStreamSchema,
});

export const ImportExpressionSchema = z.object({
  type: z.literal("ImportExpression"),
  imports: z.array(z.string()),
});

export const ExpressionSchema = z.discriminatedUnion("type", [
  ImportExpressionSchema,
]);

export type Expression = z.infer<typeof ExpressionSchema>;
export type ImportExpression = z.infer<typeof ImportExpressionSchema>;
