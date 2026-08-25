import { describe, expect, it } from 'vitest';
import { render, screen, within } from '@testing-library/svelte';
import InventoryTable from '$lib/components/inventory/InventoryTable.svelte';
import { i18n } from '$lib/i18n';
import type { InventoryRecord } from '$lib/types';

const baseRecord: InventoryRecord = {
	id: 'codex:mcp:/Users/test/.codex/config.toml:docs:0',
	client: 'codex',
	itemType: 'mcp',
	name: 'docs',
	scope: 'user',
	sourceKind: 'userConfig',
	sourcePath: '/Users/test/.codex/config.toml',
	projectPath: null,
	originalPath: '/Users/test/.codex/config.toml',
	resolvedPath: '/Users/test/.codex/config.toml',
	isSymlink: false,
	enabled: true,
	trustState: 'notApplicable',
	isEffective: true,
	sourcePriority: 100,
	protectedFields: ['Environment variables', 'HTTP headers'],
	detail: 'STDIO MCP server'
};

describe('InventoryTable', () => {
	it('shows tool provenance without rendering protected values', () => {
		render(InventoryTable, { props: { records: [baseRecord] } });

		expect(screen.getByText('docs')).toBeInTheDocument();
		expect(screen.getByText('Codex')).toBeInTheDocument();
		expect(screen.getByText('User config')).toBeInTheDocument();
		expect(screen.getByText('2 protected field groups hidden')).toBeInTheDocument();
		expect(document.body.textContent).not.toContain('secret-token-value');
	});

	it('shows disabled and unknown-trust states explicitly', () => {
		render(InventoryTable, {
			props: {
				records: [{ ...baseRecord, id: 'cursor:hook:1', client: 'cursor', itemType: 'hook', enabled: false, trustState: 'unknown' }]
			}
		});

		expect(screen.getByText('Disabled')).toBeInTheDocument();
		expect(screen.getByText('Trust not reported')).toBeInTheDocument();
	});

	it('shows when a project has not been trusted', () => {
		render(InventoryTable, {
			props: {
				records: [{ ...baseRecord, id: 'claude:mcp:untrusted', client: 'claude', trustState: 'untrusted', isEffective: false }]
			}
		});

		expect(screen.getByText('Project not trusted')).toBeInTheDocument();
		expect(screen.getByText('Not effective')).toBeInTheDocument();
	});

	it('labels hook trust separately from project trust', () => {
		render(InventoryTable, {
			props: {
				records: [
					{ ...baseRecord, id: 'codex:hook:trusted', name: 'trusted-user-hook', itemType: 'hook', trustState: 'trusted' },
					{ ...baseRecord, id: 'codex:hook:untrusted', name: 'untrusted-plugin-hook', itemType: 'hook', sourceKind: 'pluginConfig', trustState: 'untrusted', isEffective: false },
					{ ...baseRecord, id: 'claude:hook:trusted', name: 'trusted-claude-hook', client: 'claude', itemType: 'hook', trustState: 'trusted' },
					{ ...baseRecord, id: 'claude:mcp:project', name: 'untrusted-project-mcp', client: 'claude', scope: 'project', trustState: 'untrusted', isEffective: false }
				]
			}
		});

		const trustedHookRow = screen.getByText('trusted-user-hook').closest('tr');
		const untrustedHookRow = screen.getByText('untrusted-plugin-hook').closest('tr');
		const claudeHookRow = screen.getByText('trusted-claude-hook').closest('tr');
		const projectRow = screen.getByText('untrusted-project-mcp').closest('tr');

		expect(trustedHookRow).not.toBeNull();
		expect(untrustedHookRow).not.toBeNull();
		expect(claudeHookRow).not.toBeNull();
		expect(projectRow).not.toBeNull();
		expect(within(trustedHookRow!).getByText('Hook trusted')).toBeInTheDocument();
		expect(within(untrustedHookRow!).getByText('Hook not trusted')).toBeInTheDocument();
		expect(within(claudeHookRow!).getByText('Project trusted')).toBeInTheDocument();
		expect(within(projectRow!).getByText('Project not trusted')).toBeInTheDocument();
	});

	it('keeps same-named tools from separate sources visible', () => {
		render(InventoryTable, {
			props: {
				records: [baseRecord, { ...baseRecord, id: 'claude:mcp:docs:0', client: 'claude', sourcePath: '/project/.mcp.json' }]
			}
		});

		expect(screen.getAllByText('docs')).toHaveLength(2);
	});

	it('renders inventory labels in the selected language', () => {
		i18n.setLocale('zh-CN');
		try {
			render(InventoryTable, { props: { records: [baseRecord] } });
			expect(screen.getByRole('columnheader', { name: '工具' })).toBeInTheDocument();
			expect(screen.getByText('用户配置')).toBeInTheDocument();
			expect(screen.getByText('已启用')).toBeInTheDocument();
		} finally {
			i18n.setLocale('en');
		}
	});
});
