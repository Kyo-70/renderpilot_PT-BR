<script lang="ts">
  import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';

  import { t } from '@shared/i18n';
  import {
    Button,
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
  } from '@shared/ui';

  type AddonActionTone = 'default' | 'warning' | 'destructive';

  type Props = {
    open: boolean;
    busy: boolean;
    tone: AddonActionTone;
    title: string;
    description: string;
    warning?: string;
    confirmLabel: string;
    onOpenChange: (open: boolean) => void;
    onConfirm: () => void;
  };

  const {
    open,
    busy,
    tone,
    title,
    description,
    warning = '',
    confirmLabel,
    onOpenChange,
    onConfirm,
  }: Props = $props();

  function requestOpenChange(nextOpen: boolean): void {
    if (!busy || nextOpen) {
      onOpenChange(nextOpen);
    }
  }
</script>

<Dialog {open} onOpenChange={requestOpenChange}>
  <DialogContent closeLabel={t('common.close')} class="sm:max-w-md">
    <DialogHeader>
      <DialogTitle>{title}</DialogTitle>
      <DialogDescription>{description}</DialogDescription>
    </DialogHeader>

    {#if warning}
      <div
        role="alert"
        class:border-destructive={tone === 'destructive'}
        class:text-destructive={tone === 'destructive'}
        class="flex gap-2 rounded-md border border-warning/40 bg-warning/10 p-3 text-sm"
      >
        <TriangleAlertIcon class="mt-0.5 size-4 shrink-0" aria-hidden="true" />
        <p class="min-w-0 whitespace-pre-line">{warning}</p>
      </div>
    {/if}

    <DialogFooter>
      <Button
        type="button"
        variant="secondary"
        size="sm"
        disabled={busy}
        onclick={() => {
          requestOpenChange(false);
        }}
      >
        {t('common.cancel')}
      </Button>
      <Button
        type="button"
        variant={tone === 'destructive' ? 'destructive' : 'default'}
        size="sm"
        disabled={busy}
        onclick={onConfirm}
      >
        {confirmLabel}
      </Button>
    </DialogFooter>
  </DialogContent>
</Dialog>
