<script lang="ts">
	import { Link, ShieldCheck } from 'lucide-svelte';
	import type {
		InventoryClient,
		InventoryItemType,
		InventoryRecord,
		InventoryScope,
		InventorySourceKind
	} from '$lib/types';

	type Props = { records: InventoryRecord[] };
	let { records }: Props = $props();

	const clientLabels: Record<InventoryClient, string> = {
		claude: 'Claude',
		codex: 'Codex',
		cursor: 'Cursor'
	};
	const itemLabels: Record<InventoryItemType, string> = {
		skill: 'Skill',
		mcp: 'MCP',
		hook: 'Hook'
	};
	const scopeLabels: Record<InventoryScope, string> = {
		user: 'User',
		project: 'Project',
		admin: 'Admin',
		legacy: 'Legacy'
	};
	const sourceLabels: Record<InventorySourceKind, string> = {
		userConfig: 'User config',
		projectConfig: 'Project config',
		localConfig: 'Local config',
		userSkills: 'User skills',
		projectSkills: 'Project skills',
		adminSkills: 'Admin skills',
		legacySkills: 'Legacy skills'
	};

	function statusLabel(record: InventoryRecord): string {
		if (record.enabled === null) return 'Status not reported';
		return record.enabled ? 'Enabled' : 'Disabled';
	}
</script>

<div class="card overflow-hidden p-0">
	<div class="overflow-x-auto">
		<table class="w-full text-left text-sm">
			<thead class="bg-gray-50 text-xs uppercase tracking-wide text-gray-500 dark:bg-gray-800/70 dark:text-gray-400">
				<tr>
					<th class="px-4 py-3 font-semibold" scope="col">Tool</th>
					<th class="px-4 py-3 font-semibold" scope="col">Client</th>
					<th class="px-4 py-3 font-semibold" scope="col">Source</th>
					<th class="px-4 py-3 font-semibold" scope="col">Status</th>
				</tr>
			</thead>
			<tbody class="divide-y divide-gray-200 dark:divide-gray-700">
				{#each records as record (record.id)}
					<tr class="align-top text-gray-700 dark:text-gray-300">
						<td class="px-4 py-4">
							<div class="font-medium text-gray-900 dark:text-white">{record.name}</div>
							<div class="mt-1 flex flex-wrap items-center gap-1.5 text-xs text-gray-500 dark:text-gray-400">
								<span class="rounded bg-gray-100 px-1.5 py-0.5 dark:bg-gray-700">{itemLabels[record.itemType]}</span>
								{#if record.detail}<span>{record.detail}</span>{/if}
							</div>
						</td>
						<td class="px-4 py-4">
							<span class="font-medium">{clientLabels[record.client]}</span>
							<div class="mt-1 text-xs text-gray-500 dark:text-gray-400">{scopeLabels[record.scope]} scope</div>
						</td>
						<td class="max-w-xl px-4 py-4">
							<div>{sourceLabels[record.sourceKind]}</div>
							<details class="mt-1 text-xs text-gray-500 dark:text-gray-400">
								<summary class="cursor-pointer select-none">Show location</summary>
								<div class="mt-2 space-y-1.5">
									<code class="block break-all rounded bg-gray-100 px-2 py-1 dark:bg-gray-800">{record.sourcePath}</code>
									{#if record.projectPath}<p class="break-all">Project: {record.projectPath}</p>{/if}
									{#if record.isSymlink && record.resolvedPath}
										<p class="flex items-start gap-1 break-all"><Link class="mt-0.5 h-3 w-3 shrink-0" />Resolves to {record.resolvedPath}</p>
									{/if}
								</div>
							</details>
						</td>
						<td class="px-4 py-4">
							<span class="font-medium {record.enabled === false ? 'text-amber-700 dark:text-amber-400' : ''}">{statusLabel(record)}</span>
							{#if record.trustState === 'unknown'}
								<div class="mt-1 text-xs text-gray-500 dark:text-gray-400">Trust not reported</div>
							{:else if record.trustState === 'trusted'}
								<div class="mt-1 text-xs text-gray-500 dark:text-gray-400">Project trusted</div>
							{:else if record.trustState === 'untrusted'}
								<div class="mt-1 text-xs text-amber-700 dark:text-amber-400">Project not trusted</div>
							{/if}
							{#if record.protectedFields.length > 0}
								<div class="mt-2 flex items-start gap-1.5 text-xs text-emerald-700 dark:text-emerald-400" title={record.protectedFields.join(', ')}>
									<ShieldCheck class="mt-0.5 h-3.5 w-3.5 shrink-0" />
									<span>{record.protectedFields.length} protected field {record.protectedFields.length === 1 ? 'group' : 'groups'} hidden</span>
								</div>
							{/if}
						</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
</div>
