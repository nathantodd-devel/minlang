import { GenericError } from "../error";

export class InvalidBitsize extends GenericError {
  override readonly name = "InvalidBitsize";

  constructor(bitsize: string, line: number, column: number, file?: string) {
    super(
      `Invalid bitsize '${bitsize}', expected one of [8, 16, 32, 64]`,
      line,
      column,
      file,
    );
  }

  override format(): string {
    return `${this.file ?? "stdin"}:${this.line}:${this.column}: ${this.message}`;
  }
}

export class UnexpectedCharacterError extends GenericError {
  override readonly name = "UnexpectedCharacterError";

  constructor(
    public readonly char: string,
    line: number,
    column: number,
    file?: string,
  ) {
    super(`Unexpected character '${char}'`, line, column, file);
  }

  override format(): string {
    return `${this.file ?? "stdin"}:${this.line}:${this.column}: ${this.message}`;
  }
}

export class InvalidNumericLiteralError extends GenericError {
  override readonly name = "InvalidNumericLiteralError";

  constructor(message: string, line: number, column: number, file?: string) {
    super(message, line, column, file);
  }

  override format(): string {
    return `${this.file ?? "stdin"}:${this.line}:${this.column}: ${this.message}`;
  }
}

export class UnterminatedStringError extends GenericError {
  override readonly name = "UnterminatedStringError";

  constructor(line: number, column: number, file?: string) {
    super("Unterminated string literal", line, column, file);
  }

  override format(): string {
    return `${this.file ?? "stdin"}:${this.line}:${this.column}: ${this.message}`;
  }
}
