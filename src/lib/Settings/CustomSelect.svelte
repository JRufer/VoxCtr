<script lang="ts">
  import { onMount } from "svelte";

  let {
    value = $bindable(),
    options = [],
    disabled = false,
    onchange
  }: {
    value: any;
    options: (string | { value: any; label: string; disabled?: boolean })[];
    disabled?: boolean;
    onchange?: (val: any) => void;
  } = $props();

  let isOpen = $state(false);
  let containerEl = $state<HTMLDivElement | null>(null);

  let normalizedOptions = $derived(
    options.map(opt => {
      if (typeof opt === "string") {
        return { value: opt, label: opt, disabled: false };
      }
      return {
        value: opt.value,
        label: opt.label,
        disabled: !!opt.disabled
      };
    })
  );

  let selectedLabel = $derived.by(() => {
    const selected = normalizedOptions.find(opt => opt.value === value);
    return selected ? selected.label : (value !== undefined && value !== null ? String(value) : "");
  });

  // Click outside listener using Svelte 5 effect
  $effect(() => {
    if (!isOpen) return;

    const handleClickOutside = (event: MouseEvent) => {
      if (containerEl && !containerEl.contains(event.target as Node)) {
        isOpen = false;
      }
    };

    document.addEventListener("click", handleClickOutside);
    return () => {
      document.removeEventListener("click", handleClickOutside);
    };
  });

  function selectOption(val: any) {
    value = val;
    isOpen = false;
    if (onchange) {
      onchange(val);
    }
  }
</script>

<div class="custom-select-wrapper" bind:this={containerEl}>
  <button
    type="button"
    class="custom-select-trigger"
    {disabled}
    onclick={() => isOpen = !isOpen}
  >
    {selectedLabel}
  </button>
  {#if isOpen && !disabled}
    <div class="custom-dropdown-menu">
      {#each normalizedOptions as opt}
        <button
          type="button"
          class="custom-dropdown-item"
          disabled={opt.disabled}
          class:selected={opt.value === value}
          onclick={(e) => {
            e.preventDefault();
            e.stopPropagation();
            selectOption(opt.value);
          }}
        >
          {opt.label}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  @reference "../../app.css";

  .custom-select-wrapper {
    @apply relative flex-[2] min-w-[190px] w-full;
  }

  .custom-select-trigger {
    @apply w-full text-left bg-[var(--surface2)] text-[var(--text)] border border-[var(--border)] rounded-[var(--radius)] p-2 pr-9 cursor-pointer transition-all duration-150 ease-out shadow-[0_4px_12px_rgba(0,0,0,0.15)] select-none text-[13px] font-semibold;
    background: var(--surface2) url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12' fill='none' stroke='%239ca3af' stroke-width='2' stroke-linecap='round' stroke-linejoin='round'><path d='m3 5 3 3 3-3'/></svg>") no-repeat right 12px center;
    background-size: 10px;
  }
  .custom-select-trigger:hover {
    @apply border-white/12 bg-[var(--color-obsidian-700)] -translate-y-[1px];
  }
  .custom-select-trigger:focus {
    @apply border-[var(--accent2)] shadow-[0_0_0_2px_rgba(56,189,248,0.15)];
  }
  .custom-select-trigger:disabled {
    @apply opacity-50 cursor-not-allowed -translate-y-0!;
  }

  .custom-dropdown-menu {
    @apply absolute left-0 right-0 mt-1 max-h-60 overflow-y-auto bg-[#1e2436] border border-[var(--border)] rounded-[var(--radius)] shadow-[0_10px_30px_rgba(0,0,0,0.5)] z-[200] flex flex-col p-1 gap-0.5;
  }

  .custom-dropdown-item {
    @apply w-full text-left bg-transparent border-none rounded-[var(--radius)] p-2 px-3 text-[13px] text-[var(--text)] cursor-pointer transition-colors duration-150;
  }
  .custom-dropdown-item:hover {
    @apply bg-white/[0.04];
  }
  .custom-dropdown-item.selected {
    @apply bg-[var(--accent2)]/10 text-[var(--accent2)] font-semibold;
  }
  .custom-dropdown-item:disabled {
    @apply opacity-40 cursor-not-allowed;
  }
  .custom-dropdown-item:disabled:hover {
    @apply bg-transparent;
  }
</style>
