<script lang="ts">
  let { message }: { message: string } = $props();
</script>

<div class="trip" role="alert">{message}</div>

<style>
  .trip {
    background: var(--trip);
    color: #f7f1ea;
    padding: 8px 16px;
    font-size: 13px;
  }
</style>
