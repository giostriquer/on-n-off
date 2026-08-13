<script lang="ts">
  import { Dialog } from "@skeletonlabs/skeleton-svelte";
  import { copy } from "./copy";

  let {
    title,
    body,
    confirmLabel = copy.uninstall,
    busy = false,
    onCancel,
    onConfirm,
  }: {
    title: string;
    body: string;
    confirmLabel?: string;
    busy?: boolean;
    onCancel: () => void;
    onConfirm: () => void;
  } = $props();
</script>

<Dialog
  role="alertdialog"
  open
  onOpenChange={(details) => {
    if (!details.open) {
      onCancel();
    }
  }}
>
  <Dialog.Backdrop class="fixed inset-0 z-50 bg-black/55" />
  <Dialog.Positioner class="fixed inset-0 z-50 flex items-center justify-center p-4">
    <Dialog.Content
      class="w-[410px] max-w-[calc(100vw-32px)] rounded-xl border border-[var(--hair)] bg-[var(--plate)] p-[18px] shadow-[0_24px_60px_rgba(0,0,0,.5)]"
    >
      <Dialog.Title class="text-[15px] font-semibold leading-snug">{title}</Dialog.Title>
      <Dialog.Description class="mt-2 text-[13px] text-[var(--mute)]">{body}</Dialog.Description>
      <footer class="mt-4 flex justify-end gap-2">
        <Dialog.CloseTrigger
          class="h-8 rounded-lg border border-[var(--hair)] bg-[var(--well)] px-3.5 text-[12.5px] text-[var(--silkscreen)]"
        >
          {copy.cancel}
        </Dialog.CloseTrigger>
        <button
          type="button"
          class="h-8 rounded-lg border border-[var(--trip)] bg-[var(--trip)] px-3.5 text-[12.5px] text-[#f7f1ea] disabled:opacity-45"
          disabled={busy}
          onclick={onConfirm}
        >
          {confirmLabel}
        </button>
      </footer>
    </Dialog.Content>
  </Dialog.Positioner>
</Dialog>
