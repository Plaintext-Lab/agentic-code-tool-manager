import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/svelte';
import { invoke } from '@tauri-apps/api/core';
import InventoryPage from '../../routes/inventory/+page.svelte';
import { i18n } from '$lib/stores';
import type { InventoryRecord, InventorySnapshot } from '$lib/types';

const skill: InventoryRecord = {
	id: 'codex:skill:user:toggle-me',
	client: 'codex',
	itemType: 'skill',
	name: 'toggle-me',
	scope: 'user',
	sourceKind: 'userSkills',
	sourcePath: '/Users/test/.agents/skills/toggle-me/SKILL.md',
	projectPath: null,
	originalPath: '/Users/test/.agents/skills/toggle-me/SKILL.md',
	resolvedPath: '/Users/test/.agents/skills/toggle-me/SKILL.md',
	isSymlink: false,
	enabled: true,
	trustState: 'notApplicable',
	isEffective: true,
	sourcePriority: 100,
	protectedFields: [],
	detail: null,
	actionCapabilities: {
		enable: { available: false, blockedReason: 'alreadyEnabled' },
		disable: { available: true, blockedReason: null },
		confirmationRequired: true,
		reloadGuidance: 'restartClient',
		sourceRevision: 'sha256:exact-revision'
	}
};

const snapshot: InventorySnapshot = {
	records: [skill],
	warnings: [],
	capabilities: [{ client: 'codex', skills: true, mcps: true, hooks: true }],
	scannedProjectCount: 0
};

const mcp: InventoryRecord = {
	...skill,
	id: 'codex:mcp:user:docs',
	itemType: 'mcp',
	name: 'docs',
	sourceKind: 'userConfig',
	sourcePath: '/Users/test/.codex/config.toml',
	originalPath: '/Users/test/.codex/config.toml',
	resolvedPath: '/Users/test/.codex/config.toml',
	protectedFields: ['Environment variables'],
	detail: 'STDIO MCP server'
};

const mcpSnapshot: InventorySnapshot = { ...snapshot, records: [mcp] };

describe('Inventory page actions', () => {
	beforeEach(() => {
		vi.mocked(invoke).mockReset();
	});

	it('sends only the record id, desired state, and source revision', async () => {
		vi.mocked(invoke).mockImplementation(async (command) => {
			if (command === 'get_tool_inventory') return snapshot;
			throw 'The inventory changed after it was scanned. Scan again and retry.';
		});
		render(InventoryPage);
		await screen.findByText('toggle-me');

		await fireEvent.click(screen.getByRole('button', { name: 'Disable toggle-me' }));
		await fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Disable' }));

		await waitFor(() => {
			expect(invoke).toHaveBeenCalledWith('set_inventory_record_enabled', {
				recordId: skill.id,
				enabled: false,
				sourceRevision: 'sha256:exact-revision'
			});
		});
	});

	it('keeps the prior inventory visible and gives a rescan action after failure', async () => {
		vi.mocked(invoke).mockImplementation(async (command) => {
			if (command === 'get_tool_inventory') return snapshot;
			throw 'The inventory changed after it was scanned. Scan again and retry.';
		});
		render(InventoryPage);
		await screen.findByText('toggle-me');

		await fireEvent.click(screen.getByRole('button', { name: 'Disable toggle-me' }));
		await fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Disable' }));

		const alert = await screen.findByRole('alert');
		expect(within(alert).getByText('Skill state was not changed')).toBeInTheDocument();
		expect(within(alert).getByRole('button', { name: 'Scan again' })).toBeInTheDocument();
		expect(screen.getByText('toggle-me')).toBeInTheDocument();
		expect(screen.getByText('Enabled')).toBeInTheDocument();
	});

	it('reports an MCP-specific failure while keeping its prior state visible', async () => {
		vi.mocked(invoke).mockImplementation(async (command) => {
			if (command === 'get_tool_inventory') return mcpSnapshot;
			throw 'The inventory changed after it was scanned. Scan again and retry.';
		});
		render(InventoryPage);
		await screen.findByText('docs');

		await fireEvent.click(screen.getByRole('button', { name: 'Disable docs' }));
		await fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Disable' }));

		const alert = await screen.findByRole('alert');
		expect(within(alert).getByText('MCP server state was not changed')).toBeInTheDocument();
		expect(within(alert).getByText('The MCP server could not be updated. Scan again and retry.')).toBeInTheDocument();
		expect(screen.getByText('docs')).toBeInTheDocument();
		expect(screen.getByText('Enabled')).toBeInTheDocument();
	});

	it('renders the freshly read Codex MCP state after a successful action', async () => {
		const disabledMcp: InventoryRecord = {
			...mcp,
			enabled: false,
			isEffective: false,
			actionCapabilities: {
				...mcp.actionCapabilities,
				enable: { available: true, blockedReason: null },
				disable: { available: false, blockedReason: 'alreadyDisabled' },
				sourceRevision: 'sha256:updated-revision'
			}
		};
		vi.mocked(invoke).mockImplementation(async (command) => {
			if (command === 'get_tool_inventory') return mcpSnapshot;
			if (command === 'set_inventory_record_enabled') {
				return { ...mcpSnapshot, records: [disabledMcp] };
			}
			throw new Error(`Unexpected command: ${command}`);
		});
		render(InventoryPage);
		await screen.findByText('docs');

		await fireEvent.click(screen.getByRole('button', { name: 'Disable docs' }));
		await fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Disable' }));

		await waitFor(() => {
			expect(invoke).toHaveBeenCalledWith('set_inventory_record_enabled', {
				recordId: mcp.id,
				enabled: false,
				sourceRevision: 'sha256:exact-revision'
			});
		});
		expect(await screen.findByText('Disabled')).toBeInTheDocument();
		expect(screen.getByRole('button', { name: 'Enable docs' })).toBeInTheDocument();
	});

	it('clears old action banners when a fresh scan succeeds', async () => {
		vi.mocked(invoke).mockImplementation(async (command) => {
			if (command === 'get_tool_inventory') return snapshot;
			throw 'The inventory changed after it was scanned. Scan again and retry.';
		});
		render(InventoryPage);
		await screen.findByText('toggle-me');
		await fireEvent.click(screen.getByRole('button', { name: 'Disable toggle-me' }));
		await fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: 'Disable' }));
		const alert = await screen.findByRole('alert');

		await fireEvent.click(within(alert).getByRole('button', { name: 'Scan again' }));

		await waitFor(() => {
			expect(screen.queryByText('Skill state was not changed')).not.toBeInTheDocument();
		});
	});

	it('uses the localized fallback instead of an English backend error', async () => {
		i18n.setLocale('zh-CN');
		try {
			vi.mocked(invoke).mockImplementation(async (command) => {
				if (command === 'get_tool_inventory') return snapshot;
				throw 'The inventory changed after it was scanned. Scan again and retry.';
			});
			render(InventoryPage);
			await screen.findByText('toggle-me');

			await fireEvent.click(screen.getByRole('button', { name: '禁用 toggle-me' }));
			await fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: '禁用' }));

			const alert = await screen.findByRole('alert');
			expect(within(alert).getByText('无法更新技能。请重新扫描后再试。')).toBeInTheDocument();
			expect(within(alert).queryByText(/inventory changed/i)).not.toBeInTheDocument();
		} finally {
			i18n.setLocale('en');
		}
	});

	it('blocks skill actions while a rescan is running', async () => {
		let finishRescan: ((value: InventorySnapshot) => void) | undefined;
		let inventoryCalls = 0;
		vi.mocked(invoke).mockImplementation(async (command) => {
			if (command !== 'get_tool_inventory') throw new Error(`Unexpected command: ${command}`);
			inventoryCalls += 1;
			if (inventoryCalls === 1) return snapshot;
			return new Promise<InventorySnapshot>((resolve) => {
				finishRescan = resolve;
			});
		});
		render(InventoryPage);
		await screen.findByText('toggle-me');

		await fireEvent.click(screen.getByRole('button', { name: 'Scan inventory again' }));

		const action = screen.getByRole('button', { name: 'Disable toggle-me' });
		expect(action).toBeDisabled();
		await fireEvent.click(action);
		expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
		finishRescan?.(snapshot);
	});
});
