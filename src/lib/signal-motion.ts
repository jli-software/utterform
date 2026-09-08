/** Presentation only: never delays recording, transcription or delivery. */
const active = (phase: string) => phase === "starting" || phase === "recording" || phase === "processing";
const bounded = (value: number) => Number.isFinite(value) ? Math.max(0, Math.min(1, value)) : 0;
const mix = (from: number, to: number, weight: number) => from + (to - from) * weight;
const settleSeconds = 1.8;

type Pose = { energy: number; activity: number; opacity: number; speed: number; flowSpeed: number };
const rest: Pose = { energy: .35, activity: 0, opacity: .5, speed: 0, flowSpeed: 0 };

export class SignalMotion {
  phase = "idle";
  time = 2.4;
  travel = 2.4;
  pose: Pose = { ...rest };
  private settling: { elapsed: number; from: Pose } | null = null;

  get moving() { return active(this.phase) || this.settling !== null; }

  setPhase(phase: string) {
    if (phase === this.phase) return;
    const wasActive = active(this.phase) || this.phase === "paused";
    this.phase = phase;
    if (active(phase) || phase === "paused") {
      // Resume/restart from the current pose and integrated particle positions.
      this.settling = null;
    } else if (wasActive && !this.settling) {
      this.settling = { elapsed: 0, from: { ...this.pose } };
    }
  }

  /** No hidden-window tail to replay; reduced motion shows a stable pose. */
  settleImmediately() {
    this.settling = null;
    if (this.phase === "paused") return;
    this.pose = active(this.phase) ? this.target(.5) : { ...rest };
  }

  private target(level: number): Pose {
    if (this.phase === "recording") {
      return { energy: .55 + level * .4, activity: level, opacity: 1, speed: .65 + level * .55, flowSpeed: .4 + level * 2.8 };
    }
    return { energy: .5, activity: .12, opacity: .72, speed: .4, flowSpeed: .35 };
  }

  advance(seconds: number, level: number) {
    if (!this.moving || this.phase === "paused") return;
    const dt = Number.isFinite(seconds) ? Math.max(0, Math.min(.1, seconds)) : 0;
    const previousSpeed = this.pose.speed;
    const previousFlow = this.pose.flowSpeed;
    if (this.settling) {
      this.settling.elapsed += dt;
      const u = Math.min(1, this.settling.elapsed / settleSeconds);
      // Smoothstep has zero slope at both ends: no abrupt release or final snap.
      const ease = u * u * (3 - 2 * u);
      for (const key of Object.keys(rest) as (keyof Pose)[]) {
        this.pose[key] = mix(this.settling.from[key], rest[key], ease);
      }
      if (u === 1) this.settling = null;
    } else {
      const target = this.target(bounded(level));
      for (const key of Object.keys(rest) as (keyof Pose)[]) {
        const tau = key === "activity" ? (target.activity > this.pose.activity ? .09 : .3) : .28;
        this.pose[key] = mix(this.pose[key], target[key], 1 - Math.exp(-dt / tau));
      }
    }
    // Integrate speeds; multiplying absolute time by a live input would teleport particles.
    this.time += dt * (previousSpeed + this.pose.speed) / 2;
    this.travel += dt * (previousFlow + this.pose.flowSpeed) / 2;
  }
}
