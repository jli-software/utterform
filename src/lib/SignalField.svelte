<script lang="ts">
  import { onMount } from "svelte";
  import { drawSignal } from "./signal";

  export let level = 0;
  export let phase = "idle";
  let field: HTMLCanvasElement;
  let context: CanvasRenderingContext2D | null = null;
  let mounted = false;
  let reduced = false;
  let visible = true;
  let frame = 0;
  let previous = 0;
  let time = 2.4;
  let energy = .35;
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
    drawSignal(context, width, height, time, energy, ink);
  }
  function tick(stamp: number) {
    frame = 0;
    // At most 30fps. Idle, paused and hidden windows have no animation loop.
    if (!previous || stamp - previous >= 1000 / 30) {
      const elapsed = previous ? Math.min((stamp - previous) / 1000, .1) : 0;
      previous = stamp;
      time += elapsed;
      const envelope = Number.isFinite(level) ? Math.max(0, Math.min(1, level)) : 0;
      const target = phase === "recording" ? .55 + envelope * .4 : .72;
      energy += (target - energy) * .18;
      draw();
    }
    frame = requestAnimationFrame(tick);
  }
  function sync(current: string, reduceMotion: boolean, isVisible: boolean) {
    if (!mounted || !context) return;
    const active = current === "recording" || current === "processing" || current === "starting";
    const moving = active && !reduceMotion && isVisible;
    if (moving && !frame) { previous = 0; frame = requestAnimationFrame(tick); }
    if (!moving) {
      cancelAnimationFrame(frame);
      frame = 0;
      if (current !== "paused") energy = active ? .8 : .35;
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

<canvas bind:this={field} class="signal-field" class:quiet={phase !== "recording" && phase !== "paused"} aria-hidden="true"></canvas>
