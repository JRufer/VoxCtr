<script lang="ts">
  import { onMount } from "svelte";
  import Settings from "./lib/Settings/Settings.svelte";
  import Overlay from "./lib/Overlay/Overlay.svelte";
  import History from "./lib/History/History.svelte";
  import UdevWarning from "./lib/Diagnostics/UdevWarning.svelte";
  import SetupWizard from "./lib/Wizard/SetupWizard.svelte";

  // Determine which view to render based on the URL path
  const path = window.location.pathname;

  function getView() {
    if (path.startsWith("/overlay")) return "overlay";
    if (path.startsWith("/history")) return "history";
    if (path.startsWith("/udev-warning")) return "udev-warning";
    if (path.startsWith("/wizard")) return "wizard";
    return "settings";
  }

  const view = getView();
  if (view === "overlay") {
    document.documentElement.classList.add("overlay-window");
    document.body.classList.add("overlay-window");
  }
</script>

{#if view === "overlay"}
  <Overlay />
{:else if view === "history"}
  <History />
{:else if view === "udev-warning"}
  <UdevWarning />
{:else if view === "wizard"}
  <SetupWizard />
{:else}
  <Settings />
{/if}
