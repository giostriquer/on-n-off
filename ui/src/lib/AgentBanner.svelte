<script lang="ts">
  let { message }: { message: string } = $props();
</script>

<div class="bg-[var(--trip)] px-4 py-2 text-[13px] text-[#f7f1ea]" role="alert">{message}</div>
