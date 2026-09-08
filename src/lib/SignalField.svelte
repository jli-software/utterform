<script lang="ts">
  import { onMount } from "svelte";
  import { signalPaths } from "./signal";

  export let level = 0;
  export let phase = "idle";
  let field: SVGSVGElement;
  let mounted = false;
  let reduced = false;
  let visible = true;
  let frame = 0;
  let previous = 0;
  let time = 1.8;
  let energy = 0;
  const initial = signalPaths(time, 0);

  function draw() {
    const paths = signalPaths(time, energy);
    field.querySelectorAll("path").forEach((path, index) => path.setAttribute("d", paths[index]));
  }
  function tick(stamp: number) {
    frame = 0;
    // Cap decorative work at 30fps; hidden windows never run this loop.
    if (!previous || stamp - previous >= 32) {
      const elapsed = previous ? Math.min((stamp - previous) / 1000, .1) : 0;
      previous = stamp;
      time += elapsed;
      const target = phase === "recording" ? .22 + Math.max(0, Math.min(1, level || 0)) * .78 : .3;
      energy += (target - energy) * .18;
      draw();
    }
    frame = requestAnimationFrame(tick);
  }
  function sync(current: string, reduceMotion: boolean, isVisible: boolean) {
    if (!mounted) return;
    const moving = (current === "recording" || current === "processing" || current === "starting") && !reduceMotion && isVisible;
    if (moving && !frame) { previous = 0; frame = requestAnimationFrame(tick); }
    if (!moving) {
      cancelAnimationFrame(frame);
      frame = 0;
      if (current !== "paused" || reduceMotion) {
        energy = current === "recording" ? .55 : 0;
        draw();
      }
    }
  }
  $: sync(phase, reduced, visible);

  onMount(() => {
    const preference = matchMedia("(prefers-reduced-motion: reduce)");
    const mediaChanged = () => { reduced = preference.matches; };
    const visibilityChanged = () => { visible = !document.hidden; };
    mediaChanged(); visibilityChanged();
    preference.addEventListener("change", mediaChanged);
    document.addEventListener("visibilitychange", visibilityChanged);
    mounted = true;
    sync(phase, reduced, visible);
    return () => {
      mounted = false;
      cancelAnimationFrame(frame);
      preference.removeEventListener("change", mediaChanged);
      document.removeEventListener("visibilitychange", visibilityChanged);
    };
  });
</script>

<svg bind:this={field} class="signal-field" class:quiet={phase !== "recording" && phase !== "paused"} viewBox="0 0 580 155" aria-hidden="true">
  {#each initial as path, index}
    <path d={path} opacity={.18 + .48 * (1 - Math.abs((index - 13) / 13) * .55)} />
  {/each}
</svg>
