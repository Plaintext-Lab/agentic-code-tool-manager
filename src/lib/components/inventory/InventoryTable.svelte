<script lang="ts">
	import { Link, ShieldCheck } from 'lucide-svelte';
	import { i18n } from '$lib/stores';
	import { blockedReasonLabels, inventoryClientLabels } from '$lib/inventory/actionLabels';
	import type { TranslationKey } from '$lib/i18n';
	import type {
		InventoryClient,
		InventoryItemType,
		InventoryRecord,
		InventoryScope,
		InventorySourceKind
	} from '$lib/types';

	type Props = {
		records: InventoryRecord[];
		busyRecordId?: string | null;
		actionsDisabled?: boolean;
		onAction?: (record: InventoryRecord, enabled: boolean) => void;
	};
	let { records, busyRecordId = null, actionsDisabled = false, onAction }: Props = $props();

	const itemLabels: Record<InventoryItemType, TranslationKey> = {
		skill: 'inventory.skill',
		mcp: 'inventory.mcp',
		hook: 'inventory.hook'
	};
	const scopeLabels: Record<InventoryScope, TranslationKey> = {
		user: 'inventory.scopeUser',
		project: 'inventory.scopeProject',
		admin: 'inventory.scopeAdmin',
		legacy: 'inventory.scopeLegacy'
	};
	const sourceLabels: Record<InventorySourceKind, TranslationKey> = {
		userConfig: 'inventory.sourceUserConfig',
		projectConfig: 'inventory.sourceProjectConfig',
		localConfig: 'inventory.sourceLocalConfig',
		managedConfig: 'inventory.sourceManagedConfig',
		userSkills: 'inventory.sourceUserSkills',
		projectSkills: 'inventory.sourceProjectSkills',
		adminSkills: 'inventory.sourceAdminSkills',
		legacySkills: 'inventory.sourceLegacySkills',
		pluginConfig: 'inventory.sourcePluginConfig',
		pluginSkills: 'inventory.sourcePluginSkills'
	};
	function statusLabel(record: InventoryRecord): string {
		if (record.enabled === null) return i18n.t('inventory.statusNotReported');
		if (!record.enabled) return i18n.t('inventory.statusDisabled');
		if (record.isEffective === false) return i18n.t('inventory.statusNotEffective');
		if (record.isEffective === null) return i18n.t('inventory.statusContextual');
		return i18n.t('inventory.statusEnabled');
	}

	function detailLabel(detail: string | null): string | null {
		if (!detail) return null;
		const labels: Record<string, TranslationKey> = {
			'HTTP MCP server': 'inventory.detailHttpMcp',
			'STDIO MCP server': 'inventory.detailStdioMcp',
			'MCP server': 'inventory.detailMcp',
			'command handler': 'inventory.detailCommandHandler',
			'prompt handler': 'inventory.detailPromptHandler',
			'agent handler': 'inventory.detailAgentHandler',
			'http handler': 'inventory.detailHttpHandler',
			'mcp_tool handler': 'inventory.detailMcpToolHandler'
		};
		return labels[detail] ? i18n.t(labels[detail]) : detail;
	}

	function trustLabel(record: InventoryRecord, trusted: boolean): string {
		if (record.client === 'codex' && record.itemType === 'hook' && record.scope !== 'project') {
			return i18n.t(trusted ? 'inventory.hookTrusted' : 'inventory.hookNotTrusted');
		}
		return i18n.t(trusted ? 'inventory.projectTrusted' : 'inventory.projectNotTrusted');
	}

	function actionLabel(record: InventoryRecord): string {
		const { enable, disable } = record.actionCapabilities;
		if (enable.available) return i18n.t('inventory.actionCanEnable');
		if (disable.available) return i18n.t('inventory.actionCanDisable');
		const reason = enable.blockedReason ?? disable.blockedReason ?? 'stateUnavailable';
		return i18n.t(blockedReasonLabels[reason], { client: inventoryClientLabels[record.client] });
	}

	function desiredState(record: InventoryRecord): boolean | null {
		if (record.client !== 'codex' || record.itemType !== 'skill') return null;
		if (record.actionCapabilities.enable.available) return true;
		if (record.actionCapabilities.disable.available) return false;
		return null;
	}
</script>

<div class="card overflow-hidden p-0">
	<div class="overflow-x-auto">
		<table class="w-full text-left text-sm">
			<thead class="bg-gray-50 text-xs uppercase tracking-wide text-gray-500 dark:bg-gray-800/70 dark:text-gray-400">
				<tr>
					<th class="px-4 py-3 font-semibold" scope="col">{i18n.t('inventory.columnTool')}</th>
					<th class="px-4 py-3 font-semibold" scope="col">{i18n.t('inventory.columnClient')}</th>
					<th class="px-4 py-3 font-semibold" scope="col">{i18n.t('inventory.columnSource')}</th>
					<th class="px-4 py-3 font-semibold" scope="col">{i18n.t('inventory.columnStatus')}</th>
				</tr>
			</thead>
			<tbody class="divide-y divide-gray-200 dark:divide-gray-700">
				{#each records as record (record.id)}
					{@const desired = desiredState(record)}
					<tr class="align-top text-gray-700 dark:text-gray-300">
						<td class="px-4 py-4">
							<div class="font-medium text-gray-900 dark:text-white">{record.name}</div>
							<div class="mt-1 flex flex-wrap items-center gap-1.5 text-xs text-gray-500 dark:text-gray-400">
								<span class="rounded bg-gray-100 px-1.5 py-0.5 dark:bg-gray-700">{i18n.t(itemLabels[record.itemType])}</span>
								{#if detailLabel(record.detail)}<span>{detailLabel(record.detail)}</span>{/if}
							</div>
						</td>
						<td class="px-4 py-4">
							<span class="font-medium">{inventoryClientLabels[record.client]}</span>
							<div class="mt-1 text-xs text-gray-500 dark:text-gray-400">{i18n.t('inventory.scopeLabel', { scope: i18n.t(scopeLabels[record.scope]) })}</div>
						</td>
						<td class="max-w-xl px-4 py-4">
							<div>{i18n.t(sourceLabels[record.sourceKind])}</div>
							<details class="mt-1 text-xs text-gray-500 dark:text-gray-400">
								<summary class="cursor-pointer select-none">{i18n.t('inventory.showLocation')}</summary>
								<div class="mt-2 space-y-1.5">
									<code class="block break-all rounded bg-gray-100 px-2 py-1 dark:bg-gray-800">{record.sourcePath}</code>
									{#if record.projectPath}<p class="break-all">{i18n.t('inventory.projectLabel')}: {record.projectPath}</p>{/if}
									{#if record.isSymlink && record.resolvedPath}
										<p class="flex items-start gap-1 break-all"><Link class="mt-0.5 h-3 w-3 shrink-0" />{i18n.t('inventory.resolvesTo')} {record.resolvedPath}</p>
									{/if}
								</div>
							</details>
						</td>
						<td class="px-4 py-4">
							<span class="font-medium {record.enabled === false || record.isEffective === false ? 'text-amber-700 dark:text-amber-400' : ''}">{statusLabel(record)}</span>
							{#if record.trustState === 'unknown'}
								<div class="mt-1 text-xs text-gray-500 dark:text-gray-400">{i18n.t('inventory.trustNotReported')}</div>
							{:else if record.trustState === 'trusted'}
								<div class="mt-1 text-xs text-gray-500 dark:text-gray-400">{trustLabel(record, true)}</div>
							{:else if record.trustState === 'untrusted'}
								<div class="mt-1 text-xs text-amber-700 dark:text-amber-400">{trustLabel(record, false)}</div>
							{/if}
							{#if record.protectedFields.length > 0}
								<div class="mt-2 flex items-start gap-1.5 text-xs text-emerald-700 dark:text-emerald-400" title={record.protectedFields.join(', ')}>
									<ShieldCheck class="mt-0.5 h-3.5 w-3.5 shrink-0" />
									<span>{i18n.t(record.protectedFields.length === 1 ? 'inventory.protectedField' : 'inventory.protectedFields', { count: record.protectedFields.length })}</span>
								</div>
							{/if}
							<div class="mt-2 text-xs text-gray-500 dark:text-gray-400">{actionLabel(record)}</div>
							{#if desired !== null && onAction}
								<button
									class="btn btn-secondary mt-3"
									disabled={actionsDisabled || busyRecordId === record.id}
									aria-label={i18n.t(
										busyRecordId === record.id
											? (desired ? 'inventory.enablingNamed' : 'inventory.disablingNamed')
											: (desired ? 'inventory.enableNamed' : 'inventory.disableNamed'),
										{ name: record.name }
									)}
									onclick={() => onAction?.(record, desired)}
								>
									{busyRecordId === record.id
										? i18n.t(desired ? 'inventory.enabling' : 'inventory.disabling')
										: i18n.t(desired ? 'common.enable' : 'common.disable')}
								</button>
							{/if}
						</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
</div>
