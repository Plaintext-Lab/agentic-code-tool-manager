<script lang="ts">
	import { AlertTriangle } from 'lucide-svelte';
	import { blockedReasonLabels } from '$lib/inventory/actionLabels';
	import { i18n } from '$lib/stores';
	import type { InventoryWarning } from '$lib/types';

	type Props = { warnings: InventoryWarning[] };
	let { warnings }: Props = $props();

	function warningLabel(warning: InventoryWarning): string {
		if (!warning.blockedReason) return warning.message;
		return i18n.t(blockedReasonLabels[warning.blockedReason], {
			client: warning.client ? clientLabel(warning.client) : ''
		});
	}

	function clientLabel(client: NonNullable<InventoryWarning['client']>): string {
		return { claude: 'Claude', codex: 'Codex', cursor: 'Cursor' }[client];
	}
</script>

<details class="rounded-lg border border-amber-200 bg-amber-50 px-4 py-3 text-sm dark:border-amber-800 dark:bg-amber-900/20">
	<summary class="flex cursor-pointer items-center gap-2 font-medium text-amber-800 dark:text-amber-300">
		<AlertTriangle class="h-4 w-4" />{i18n.t(warnings.length === 1 ? 'inventory.sourceWarning' : 'inventory.sourceWarnings', { count: warnings.length })}
	</summary>
	<ul class="mt-3 space-y-2 text-amber-800 dark:text-amber-300">
		{#each warnings as warning}
			<li><span class="font-medium capitalize">{warning.client ? clientLabel(warning.client) : i18n.t('page.inventory.title')}:</span> {warningLabel(warning)}<code class="mt-0.5 block break-all text-xs">{warning.sourcePath}</code></li>
		{/each}
	</ul>
</details>
