import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import InventoryTable from '$lib/components/inventory/InventoryTable.svelte';
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
	});

	it('keeps same-named tools from separate sources visible', () => {
		render(InventoryTable, {
			props: {
				records: [baseRecord, { ...baseRecord, id: 'claude:mcp:docs:0', client: 'claude', sourcePath: '/project/.mcp.json' }]
			}
		});

		expect(screen.getAllByText('docs')).toHaveLength(2);
	});
});
