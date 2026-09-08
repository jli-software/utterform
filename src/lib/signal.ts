/** The approved Signal / 02 ribbon, driven by the existing microphone envelope. */
const fract = (value: number) => value - Math.floor(value);
const noise = (a: number, b: number) => fract(Math.sin(a * 127.1 + b * 311.7) * 43758.5453);

// Stable identities, sizes and drift speeds: no per-frame random flicker.
const particles = Array.from({ length: 54 }, (_, row) =>
  Array.from({ length: 186 }, (_, column) => {
    const seed = noise(row, column);
    return {
      p: column / 186, k: (row - 26.5) / 26.5,
      speed: .009 + .003 * noise(row, 2),
      brightness: .45 + .55 * seed,
      size: seed > .97 ? 1.65 : seed > .72 ? 1.05 : .65,
      include: noise(row + 91, column) <= .91,
    };
  }).filter((particle) => particle.include),
).flat();
const grains = Array.from({ length: 360 }, (_, index) => ({
  p: noise(index, 7), k: (noise(index, 9) * 2 - 1) * 1.8,
  spread: (noise(index, 13) - .5) * .27,
  alpha: .12 + .38 * noise(index, 11), size: noise(index, 12) > .9 ? 1.8 : 1,
}));

export function drawSignal(
  ctx: CanvasRenderingContext2D, width: number, height: number,
  time: number, energy: number, ink: string,
) {
  if (width <= 0 || height <= 0) return;
  const t = Number.isFinite(time) ? time : 2.4;
  const volume = Number.isFinite(energy) ? Math.max(0, Math.min(1, energy)) : .35;
  function point(p: number, k: number) {
    const envelope = Math.sin(Math.PI * p) ** .65;
    const twist = p * 8.7 - t * .5;
    const ridge = Math.sin(p * 9.1 - t * .52) * .19 + Math.sin(p * 17 + t * .31) * .045;
    const depth = Math.cos(twist + k * 1.1);
    const spread = k * (.13 + .18 * Math.sin(p * 5.2 + t * .19) ** 2) * Math.cos(twist * .62);
    return {
      x: 12 + p * (width - 24),
      y: height * .5 + envelope * height * (ridge + spread) * volume
        + Math.sin(p * 32 + k * 5 + t) * envelope * 1.1,
      depth, envelope,
    };
  }

  ctx.clearRect(0, 0, width, height);
  ctx.strokeStyle = ink;
  ctx.fillStyle = ink;
  ctx.lineWidth = .5;
  ctx.globalAlpha = .08;
  for (let row = 0; row < 38; row++) {
    const k = (row - 18.5) / 18.5;
    ctx.beginPath();
    for (let column = 0; column <= 220; column++) {
      const v = point(column / 220, k);
      if (column) ctx.lineTo(v.x, v.y);
      else ctx.moveTo(v.x, v.y);
    }
    ctx.stroke();
  }
  for (const particle of particles) {
    const v = point(fract(particle.p + t * particle.speed), particle.k);
    ctx.globalAlpha = (.14 + .65 * (v.depth * .5 + .5)) * (.3 + .7 * v.envelope) * particle.brightness;
    ctx.fillRect(v.x, v.y, particle.size, particle.size);
  }
  for (const grain of grains) {
    const v = point(fract(grain.p + t * .013), grain.k);
    ctx.globalAlpha = grain.alpha * v.envelope;
    ctx.fillRect(v.x, v.y + grain.spread * height * v.envelope, grain.size, grain.size);
  }
  ctx.globalAlpha = 1;
}
