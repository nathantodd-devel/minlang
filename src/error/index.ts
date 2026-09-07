export abstract class GenericError extends Error {
  constructor(
    message: string,
    public readonly line?: number,
    public readonly column?: number,
    public readonly file?: string,
  ) {
    super(message);
  }

  abstract format(): string;
}
