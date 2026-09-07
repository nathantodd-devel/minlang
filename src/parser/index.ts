import type { GenericError } from "../error";
import { TokenStreamSchema, type TokenStream } from "../tokenizer";

export class Parser {
  readonly tokens: TokenStream;
  readonly errors: GenericError[] = [];
  constructor(tokens: TokenStream) {
    this.tokens = TokenStreamSchema.parse(tokens);
  }

  parse() {}
}
