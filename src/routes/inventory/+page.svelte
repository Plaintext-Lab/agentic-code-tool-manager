<script lang="ts">
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { Boxes, RefreshCw, Search, ShieldCheck } from 'lucide-svelte';
	import { Header } from '$lib/components/layout';
	import InventoryTable from '$lib/components/inventory/InventoryTable.svelte';
	import InventoryActionDialog from '$lib/components/inventory/InventoryActionDialog.svelte';
	import InventoryWarningList from '$lib/components/inventory/InventoryWarningList.svelte';
	import { i18n } from '$lib/stores';
	import type { InventoryClient, InventoryItemType, InventoryRecord, InventorySnapshot } from '$lib/types';

	type ClientFilter = 'all' | InventoryClient;
	type ItemFilter = 'all' | InventoryItemType;

	let snapshot = $state<InventorySnapshot | null>(null);
	let loading = $state(true);
	let error = $state<string | null>(null);
	let actionError = $state<string | null>(null);
	let actionSuccess = $state<string | null>(null);
	let actionRecord = $state<InventoryRecord | null>(null);
	let desiredEnabled = $state(false);
	let actionLoading = $state(false);
	let search = $state('');
	let clientFilter = $state<ClientFilter>('all');
	let itemFilter = $state<ItemFilter>('all');

	const filteredRecords = $derived.by(() => {
		const query = search.trim().toLowerCase();
		return (snapshot?.records ?? []).filter((record) => {
			const matchesSearch = !query || [record.name, record.detail, record.sourcePath]
				.filter(Boolean)
				.some((value) => value?.toLowerCase().includes(query));
			return matchesSearch
				&& (clientFilter === 'all' || record.client === clientFilter)
				&& (itemFilter === 'all' || record.itemType === itemFilter);
		});
	});

	function clientCount(client: InventoryClient): number {
		return snapshot?.records.filter((record) => record.client === client).length ?? 0;
	}

	async function loadInventory() {
		if ((loading && snapshot) || actionLoading) return;
		loading = true;
		error = null;
		actionError = null;
		actionSuccess = null;
		try {
			snapshot = await invoke<InventorySnapshot>('get_tool_inventory');
		} catch (caught) {
			console.error('[Inventory] Discovery failed:', caught);
			error = typeof caught === 'string' ? caught : i18n.t('inventory.loadErrorFallback');
		} finally {
			loading = false;
		}
	}

	function requestAction(record: InventoryRecord, enabled: boolean) {
		if (actionLoading) return;
		actionError = null;
		actionSuccess = null;
		actionRecord = record;
		desiredEnabled = enabled;
	}

	async function confirmAction() {
		if (actionLoading || !actionRecord) return;
		const sourceRevision = actionRecord.actionCapabilities.sourceRevision;
		if (!sourceRevision) {
			actionError = i18n.t('inventory.actionErrorFallback');
			actionRecord = null;
			return;
		}
		actionLoading = true;
		actionError = null;
		actionSuccess = null;
		const changedRecord = actionRecord;
		try {
			snapshot = await invoke<InventorySnapshot>('set_inventory_record_enabled', {
				recordId: changedRecord.id,
				enabled: desiredEnabled,
				sourceRevision
			});
			actionSuccess = i18n.t('inventory.actionSuccess', {
				name: changedRecord.name,
				state: i18n.t(desiredEnabled ? 'common.enabled' : 'common.disabled')
			});
			actionRecord = null;
		} catch (caught) {
			console.error('[Inventory] Skill state update failed:', caught);
			actionError = i18n.t('inventory.actionErrorFallback');
			actionRecord = null;
		} finally {
			actionLoading = false;
		}
	}

	onMount(loadInventory);
</script>

<Header title={i18n.t('page.inventory.title')} subtitle={i18n.t('page.inventory.subtitle')}>
	<button class="btn btn-ghost" onclick={loadInventory} disabled={loading || actionLoading} aria-label={i18n.t('inventory.scanAgainLabel')}>
		<RefreshCw class="h-4 w-4 {loading ? 'animate-spin' : ''}" />
		{i18n.t('inventory.scanAgain')}
	</button>
</Header>

<div class="flex-1 overflow-auto p-6">
	<div class="mx-auto max-w-7xl space-y-6">
		<div class="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-emerald-200 bg-emerald-50 px-4 py-3 text-sm text-emerald-800 dark:border-emerald-800 dark:bg-emerald-900/20 dark:text-emerald-300">
			<div class="flex items-center gap-2 font-medium"><ShieldCheck class="h-4 w-4" />{i18n.t('inventory.readOnlyTitle')}</div>
			<span>{i18n.t('inventory.readOnlyDescription')}</span>
		</div>

		{#if loading && !snapshot}
			<div class="card flex items-center justify-center gap-3 py-20 text-gray-500 dark:text-gray-400" role="status">
				<RefreshCw class="h-5 w-5 animate-spin" />{i18n.t('inventory.scanning')}
			</div>
		{:else if error}
			<div class="card border-red-200 bg-red-50 p-6 dark:border-red-800 dark:bg-red-900/20" role="alert">
				<h2 class="font-semibold text-red-800 dark:text-red-300">{i18n.t('inventory.loadErrorTitle')}</h2>
				<p class="mt-1 text-sm text-red-700 dark:text-red-400">{error}</p>
				<button class="btn btn-secondary mt-4" onclick={loadInventory}>{i18n.t('inventory.tryAgain')}</button>
			</div>
		{:else if snapshot}
			{#if actionError}
				<div class="card border-red-200 bg-red-50 p-4 dark:border-red-800 dark:bg-red-900/20" role="alert">
					<h2 class="font-semibold text-red-800 dark:text-red-300">{i18n.t('inventory.actionErrorTitle')}</h2>
					<p class="mt-1 text-sm text-red-700 dark:text-red-400">{actionError}</p>
					<button class="btn btn-secondary mt-3" onclick={loadInventory}>{i18n.t('inventory.scanAgain')}</button>
				</div>
			{:else if actionSuccess}
				<div class="card border-emerald-200 bg-emerald-50 p-4 text-sm text-emerald-800 dark:border-emerald-800 dark:bg-emerald-900/20 dark:text-emerald-300" role="status">
					{actionSuccess}
				</div>
			{/if}

			<div class="grid gap-3 sm:grid-cols-3">
				{#each ['claude', 'codex', 'cursor'] as client}
					<div class="card p-4">
						<div class="text-sm font-medium capitalize text-gray-600 dark:text-gray-300">{client}</div>
						<div class="mt-1 text-2xl font-semibold text-gray-900 dark:text-white">{clientCount(client as InventoryClient)}</div>
						<div class="text-xs text-gray-500 dark:text-gray-400">{i18n.t('inventory.toolSummary')}</div>
					</div>
				{/each}
			</div>

			{#if snapshot.warnings.length > 0}
				<InventoryWarningList warnings={snapshot.warnings} />
			{/if}

			{#if snapshot.records.length === 0}
				<div class="card py-16 text-center">
					<Boxes class="mx-auto h-10 w-10 text-gray-400" />
					<h2 class="mt-3 font-semibold text-gray-900 dark:text-white">{i18n.t('inventory.noToolsTitle')}</h2>
					<p class="mt-1 text-sm text-gray-500 dark:text-gray-400">{i18n.t('inventory.noToolsDescription')}</p>
				</div>
			{:else}
				<div class="flex flex-wrap gap-3">
					<label class="relative min-w-64 flex-1">
						<span class="sr-only">{i18n.t('inventory.searchLabel')}</span>
						<Search class="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-gray-400" />
						<input class="input w-full pl-9" type="search" placeholder={i18n.t('inventory.searchPlaceholder')} bind:value={search} />
					</label>
					<label>
						<span class="sr-only">{i18n.t('inventory.filterClient')}</span>
						<select class="input" bind:value={clientFilter}>
							<option value="all">{i18n.t('inventory.allClients')}</option><option value="claude">Claude</option><option value="codex">Codex</option><option value="cursor">Cursor</option>
						</select>
					</label>
					<label>
						<span class="sr-only">{i18n.t('inventory.filterType')}</span>
						<select class="input" bind:value={itemFilter}>
							<option value="all">{i18n.t('inventory.allToolTypes')}</option><option value="skill">{i18n.t('inventory.skills')}</option><option value="mcp">{i18n.t('inventory.mcps')}</option><option value="hook">{i18n.t('inventory.hooks')}</option>
						</select>
					</label>
				</div>

				{#if filteredRecords.length === 0}
					<div class="card py-12 text-center text-sm text-gray-500 dark:text-gray-400">{i18n.t('inventory.noMatches')}</div>
				{:else}
					<div class="flex items-center justify-between text-sm text-gray-500 dark:text-gray-400">
						<span>{i18n.t('inventory.toolsCount', { shown: filteredRecords.length, total: snapshot.records.length })}</span>
						<span>{i18n.t(snapshot.scannedProjectCount === 1 ? 'inventory.projectScanned' : 'inventory.projectsScanned', { count: snapshot.scannedProjectCount })}</span>
					</div>
					<InventoryTable records={filteredRecords} busyRecordId={actionLoading ? actionRecord?.id : null} onAction={requestAction} />
				{/if}
			{/if}
		{/if}
	</div>
</div>

{#if actionRecord}
	<InventoryActionDialog
		record={actionRecord}
		enabled={desiredEnabled}
		submitting={actionLoading}
		onConfirm={confirmAction}
		onCancel={() => {
			if (!actionLoading) actionRecord = null;
		}}
	/>
{/if}
