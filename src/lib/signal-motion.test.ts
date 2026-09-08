import { describe, expect, it } from "vitest";
import { SignalMotion } from "./signal-motion";

function run(motion: SignalMotion, seconds: number, level = .65) {
  for (let i = 0; i < Math.round(seconds * 60); i++) motion.advance(1 / 60, level);
}
function recording() {
  const motion = new SignalMotion();
  motion.setPhase("recording");
  run(motion, 2);
  return motion;
}

describe("signal motion lifecycle", () => {
  it("releases continuously, reaches rest and stops after a finite tail", () => {
    const motion = recording();
    const before = { ...motion.pose };
    const time = motion.time;
    motion.setPhase("done");
    expect(motion.pose).toEqual(before);
    expect(motion.moving).toBe(true);
    motion.advance(1 / 60, 0);
    expect(motion.time).toBeGreaterThan(time);
    expect(Math.abs(motion.pose.opacity - before.opacity)).toBeLessThan(.001);
    let speed = motion.pose.speed;
    for (let i = 0; i < 120; i++) {
      motion.advance(1 / 60, 0);
      expect(motion.pose.speed).toBeLessThanOrEqual(speed);
      speed = motion.pose.speed;
    }
    expect(motion.moving).toBe(false);
    expect(motion.pose).toEqual({ energy: .35, activity: 0, opacity: .5, speed: 0, flowSpeed: 0 });
    const end = JSON.stringify(motion);
    run(motion, 5);
    expect(JSON.stringify(motion)).toBe(end);
  });

  it("eases through processing without treating stale metering as speech", () => {
    const motion = recording();
    const pose = { ...motion.pose };
    motion.setPhase("processing");
    expect(motion.pose).toEqual(pose);
    run(motion, 2, 1);
    expect(motion.pose.activity).toBeCloseTo(.12, 2);
    expect(motion.pose.speed).toBeCloseTo(.4, 2);
    expect(motion.pose.opacity).toBeCloseTo(.72, 2);
    expect(motion.moving).toBe(true);
  });

  it("makes speech drive independent flow speed and local motion", () => {
    const quiet = new SignalMotion(), speech = new SignalMotion();
    quiet.setPhase("recording"); speech.setPhase("recording");
    run(quiet, 2, 0); run(speech, 2, 1);
    expect(speech.pose.activity).toBeGreaterThan(.95);
    expect(quiet.pose.activity).toBe(0);
    expect(speech.travel - 2.4).toBeGreaterThan((quiet.travel - 2.4) * 4);
    const activity = speech.pose.activity;
    speech.advance(1 / 60, 0);
    expect(speech.pose.activity).toBeGreaterThan(activity * .9);
  });

  it("freezes pause exactly and resumes without resetting particle positions", () => {
    const motion = recording();
    motion.setPhase("paused");
    const paused = JSON.stringify(motion);
    run(motion, 2, 0);
    expect(motion.moving).toBe(false);
    expect(JSON.stringify(motion)).toBe(paused);
    const time = motion.time, travel = motion.travel;
    motion.setPhase("recording");
    expect(motion.time).toBe(time);
    expect(motion.travel).toBe(travel);
    motion.advance(1 / 60, .8);
    expect(motion.travel).toBeGreaterThan(travel);
  });

  it("can restart mid-release without teleporting or continuing the old tail", () => {
    const motion = recording();
    motion.setPhase("done"); run(motion, .8);
    const pose = { ...motion.pose }, travel = motion.travel;
    motion.setPhase("starting");
    expect(motion.pose).toEqual(pose);
    expect(motion.travel).toBe(travel);
    run(motion, 3);
    expect(motion.moving).toBe(true);
    expect(motion.pose.speed).toBeGreaterThan(.39);
  });

  it("discards a hidden/reduced-motion tail without scheduling more animation", () => {
    const motion = recording();
    motion.setPhase("error"); run(motion, .2);
    motion.settleImmediately();
    expect(motion.moving).toBe(false);
    expect(motion.pose.opacity).toBe(.5);
    const travel = motion.travel;
    run(motion, 2);
    expect(motion.travel).toBe(travel);
  });

  it("bounds invalid input and does not jump after long gaps", () => {
    const motion = recording();
    const travel = motion.travel;
    motion.advance(1000, Infinity);
    expect(motion.travel - travel).toBeLessThan(.4);
    motion.advance(NaN, NaN);
    expect(Object.values(motion.pose).every(Number.isFinite)).toBe(true);
    expect(Number.isFinite(motion.time)).toBe(true);
  });
});
