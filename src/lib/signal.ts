/** A decorative contour field, driven only by the existing normalized envelope. */
export function signalPaths(time: number, level: number): string[] {
  const energy = Number.isFinite(level) ? Math.max(0, Math.min(1, level)) : 0;
  const t = Number.isFinite(time) ? time : 0;
  return Array.from({ length: 27 }, (_, index) => {
    const k = (index - 13) / 13;
    let path = "";
    for (let x = 0; x <= 580; x += 4) {
      const p = x / 580;
      const envelope = Math.pow(Math.sin(Math.PI * p), 2.2);
      const ridge = Math.sin(p * 8.2 - t * .45 + k * .75) * 26 + Math.sin(p * 15 + t * .7) * 7;
      const spread = k * (15 + 27 * Math.pow(Math.sin(p * 5.4 + t * .2), 2));
      const y = 77 + envelope * (ridge + spread) * (.28 + energy * .72);
      path += `${x ? "L" : "M"}${x},${y.toFixed(2)}`;
    }
    return path;
  });
}
