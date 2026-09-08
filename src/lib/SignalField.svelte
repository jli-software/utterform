<script lang="ts">
  import { onMount } from "svelte";
  import { drawSignal } from "./signal";
  import { SignalMotion } from "./signal-motion";

  export let level = 0;
  export let phase = "idle";
  let field: HTMLCanvasElement;
  let context: CanvasRenderingContext2D | null = null;
  let mounted = false;
  let reduced = false;
  let visible = true;
  let frame = 0;
  let previous = 0;
  const motion = new SignalMotion();
  let ink = "#414748";
  let width = 0;
  let height = 0;

  function draw() {
    if (!context || !visible) return;
    // Bound bitmap work on Retina/HiDPI displays without reducing CSS width.
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const pixelWidth = Math.round(width * dpr);
    const pixelHeight = Math.round(height * dpr);
    if (field.width !== pixelWidth || field.height !== pixelHeight) {
      field.width = pixelWidth;
      field.height = pixelHeight;
    }
    context.setTransform(dpr, 0, 0, dpr, 0, 0);
    field.style.opacity = String(motion.pose.opacity);
    drawSignal(context, width, height, motion.time, motion.pose.energy, ink, motion.travel, motion.pose.activity);
  }
  function tick(stamp: number) {
    frame = 0;
    // At most 30fps, including the finite visible-window release tail.
    if (!previous || stamp - previous >= 1000 / 30) {
      const elapsed = previous ? (stamp - previous) / 1000 : 0;
      previous = stamp;
      motion.advance(elapsed, level);
      draw();
    }
    if (motion.moving) frame = requestAnimationFrame(tick);
  }
  function sync(current: string, reduceMotion: boolean, isVisible: boolean) {
    if (!mounted || !context) return;
    motion.setPhase(current);
    if (reduceMotion || !isVisible || current === "paused") {
      cancelAnimationFrame(frame);
      frame = 0;
      // Pausing freezes exactly. Hidden active motion resumes from where it was;
      // completed tails are discarded while hidden, without a timer or redraw.
      if (reduceMotion || (!isVisible && current !== "recording" && current !== "processing" && current !== "starting")) motion.settleImmediately();
      draw();
      return;
    }
    if (motion.moving && !frame) {
      previous = 0;
      frame = requestAnimationFrame(tick);
    } else if (!motion.moving) {
      draw();
    }
  }
  $: sync(phase, reduced, visible);

  onMount(() => {
    context = field.getContext("2d");
    if (!context) return; // Recording controls work even if canvas is unavailable.
    const preference = matchMedia("(prefers-reduced-motion: reduce)");
    const colorScheme = matchMedia("(prefers-color-scheme: dark)");
    const mediaChanged = () => { reduced = preference.matches; };
    const visibilityChanged = () => { visible = !document.hidden; };
    const themeChanged = () => { ink = getComputedStyle(field).color; draw(); };
    const resized = () => {
      width = field.clientWidth;
      height = field.clientHeight;
      draw();
    };
    const sizeObserver = new ResizeObserver(resized);
    const themeObserver = new MutationObserver(themeChanged);
    mediaChanged(); visibilityChanged();
    preference.addEventListener("change", mediaChanged);
    colorScheme.addEventListener("change", themeChanged);
    document.addEventListener("visibilitychange", visibilityChanged);
    window.addEventListener("resize", resized);
    sizeObserver.observe(field);
    themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    mounted = true;
    themeChanged(); resized();
    sync(phase, reduced, visible);
    return () => {
      mounted = false;
      cancelAnimationFrame(frame);
      sizeObserver.disconnect();
      themeObserver.disconnect();
      preference.removeEventListener("change", mediaChanged);
      colorScheme.removeEventListener("change", themeChanged);
      document.removeEventListener("visibilitychange", visibilityChanged);
      window.removeEventListener("resize", resized);
    };
  });
</script>

<canvas bind:this={field} class="signal-field" style="opacity: .5" aria-hidden="true"></canvas>
