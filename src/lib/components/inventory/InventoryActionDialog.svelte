<script lang="ts">
	import { RefreshCw, ShieldCheck } from 'lucide-svelte';
	import { tick } from 'svelte';
	import { i18n } from '$lib/stores';
	import { inventoryClientLabels } from '$lib/inventory/actionLabels';
	import type { TranslationKey } from '$lib/i18n';
	import type { InventoryRecord, InventoryScope } from '$lib/types';

	type Props = {
		record: InventoryRecord;
		enabled: boolean;
		submitting: boolean;
		onConfirm: () => void;
		onCancel: () => void;
	};

	let { record, enabled, submitting, onConfirm, onCancel }: Props = $props();

	const scopeLabels: Record<InventoryScope, TranslationKey> = {
		user: 'inventory.scopeUser',
		project: 'inventory.scopeProject',
		admin: 'inventory.scopeAdmin',
		legacy: 'inventory.scopeLegacy'
	};
	const actionName = $derived(i18n.t(enabled ? 'common.enable' : 'common.disable'));
	const title = $derived(i18n.t(enabled ? 'inventory.enableSkillTitle' : 'inventory.disableSkillTitle'));
	let dialogElement = $state<HTMLDivElement>();
	let focusTarget = $state<HTMLDivElement>();
	let previouslyFocused: HTMLElement | null = null;

	$effect(() => {
		previouslyFocused = document.activeElement as HTMLElement;
		tick().then(() => dialogElement?.querySelector<HTMLButtonElement>('[data-cancel]')?.focus());
		return () => previouslyFocused?.focus();
	});

	$effect(() => {
		if (submitting) tick().then(() => focusTarget?.focus());
	});

	function cancel() {
		if (!submitting) onCancel();
	}

	function handleKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') {
			cancel();
			return;
		}
		if (event.key !== 'Tab') return;
		const focusable = dialogElement?.querySelectorAll<HTMLElement>('button:not([disabled])');
		if (!focusable || focusable.length === 0) {
			event.preventDefault();
			focusTarget?.focus();
			return;
		}
		const first = focusable[0];
		const last = focusable[focusable.length - 1];
		if (event.shiftKey && document.activeElement === first) {
			event.preventDefault();
			last.focus();
		} else if (!event.shiftKey && document.activeElement === last) {
			event.preventDefault();
			first.focus();
		}
	}
</script>

<div
	bind:this={dialogElement}
	class="fixed inset-0 z-50 flex items-center justify-center overflow-y-auto bg-black/50 p-4"
	role="dialog"
	aria-modal="true"
	aria-labelledby="inventory-action-title"
	aria-busy={submitting}
	tabindex="-1"
	onclick={(event) => {
		if (event.target === event.currentTarget) cancel();
	}}
	onkeydown={handleKeydown}
>
	<div
		bind:this={focusTarget}
		class="max-h-[calc(100dvh-2rem)] w-full max-w-lg overflow-y-auto rounded-xl bg-white p-6 shadow-xl dark:bg-gray-800"
		role="document"
		tabindex="-1"
	>
		<div class="flex items-start gap-3">
			<div class="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-blue-100 dark:bg-blue-900/50">
				<ShieldCheck class="h-5 w-5 text-blue-600 dark:text-blue-400" aria-hidden="true" />
			</div>
			<div>
				<h2 id="inventory-action-title" class="text-lg font-semibold text-gray-900 dark:text-white">{title}</h2>
				<p class="mt-1 text-sm text-gray-500 dark:text-gray-400">{i18n.t('inventory.actionConfirmDescription')}</p>
			</div>
		</div>

		<dl class="mt-5 grid grid-cols-[auto_1fr] gap-x-4 gap-y-3 rounded-lg bg-gray-50 p-4 text-sm dark:bg-gray-900/50">
			<dt class="font-medium text-gray-500 dark:text-gray-400">{i18n.t('inventory.confirmSkill')}</dt>
			<dd class="break-all text-gray-900 dark:text-white">{record.name}</dd>
			<dt class="font-medium text-gray-500 dark:text-gray-400">{i18n.t('inventory.confirmClient')}</dt>
			<dd class="text-gray-900 dark:text-white">{inventoryClientLabels[record.client]}</dd>
			<dt class="font-medium text-gray-500 dark:text-gray-400">{i18n.t('inventory.confirmScope')}</dt>
			<dd class="text-gray-900 dark:text-white">{i18n.t('inventory.scopeLabel', { scope: i18n.t(scopeLabels[record.scope]) })}</dd>
			<dt class="font-medium text-gray-500 dark:text-gray-400">{i18n.t('inventory.confirmProject')}</dt>
			<dd class="break-all text-gray-900 dark:text-white">{record.projectPath ?? i18n.t('inventory.notApplicable')}</dd>
			<dt class="font-medium text-gray-500 dark:text-gray-400">{i18n.t('inventory.confirmDesiredState')}</dt>
			<dd class="text-gray-900 dark:text-white">{i18n.t(enabled ? 'inventory.statusEnabled' : 'inventory.statusDisabled')}</dd>
			<dt class="font-medium text-gray-500 dark:text-gray-400">{i18n.t('inventory.confirmSource')}</dt>
			<dd><code class="break-all text-xs text-gray-900 dark:text-white">{record.sourcePath}</code></dd>
		</dl>

		<p class="mt-4 text-sm text-amber-700 dark:text-amber-400">{i18n.t('inventory.restartGuidance')}</p>

		<div class="mt-6 flex justify-end gap-3">
			<button data-cancel class="btn btn-secondary" disabled={submitting} onclick={cancel}>{i18n.t('common.cancel')}</button>
			<button class="btn btn-primary" disabled={submitting} onclick={onConfirm}>
				{#if submitting}<RefreshCw class="h-4 w-4 animate-spin" />{i18n.t(enabled ? 'inventory.enabling' : 'inventory.disabling')}{:else}{actionName}{/if}
			</button>
		</div>
	</div>
</div>
