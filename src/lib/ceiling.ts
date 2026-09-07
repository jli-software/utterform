/// The last line of defence against waiting for ever.
///
/// The backend answers even when it panics, so this should never fire. It
/// exists because "never fires" is a claim about code that keeps changing, and
/// an interface stuck on "Transcribing…" with no way out is the worst outcome
/// there is. Local transcription of a long recording on a large model is slow
/// but finite, so the ceiling is generous rather than tight.
export const PROCESSING_CEILING_MS = 30 * 60_000;

export function withCeiling<T>(
  work: Promise<T>,
  message: string,
  ms: number = PROCESSING_CEILING_MS,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(message)), ms);
    work.then(resolve, reject).finally(() => clearTimeout(timer));
  });
}
