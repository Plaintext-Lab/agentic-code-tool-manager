export type InventoryClient = 'claude' | 'codex' | 'cursor';
export type InventoryItemType = 'skill' | 'mcp' | 'hook';
export type InventoryScope = 'user' | 'project' | 'admin' | 'legacy';
export type InventorySourceKind =
	| 'userConfig'
	| 'projectConfig'
	| 'localConfig'
	| 'managedConfig'
	| 'userSkills'
	| 'projectSkills'
	| 'adminSkills'
	| 'legacySkills';
export type InventoryTrustState = 'notApplicable' | 'unknown' | 'trusted' | 'untrusted';

export interface AdapterCapabilities {
	client: InventoryClient;
	skills: boolean;
	mcps: boolean;
	hooks: boolean;
}

export interface InventoryRecord {
	id: string;
	client: InventoryClient;
	itemType: InventoryItemType;
	name: string;
	scope: InventoryScope;
	sourceKind: InventorySourceKind;
	sourcePath: string;
	projectPath: string | null;
	originalPath: string;
	resolvedPath: string | null;
	isSymlink: boolean;
	enabled: boolean | null;
	trustState: InventoryTrustState;
	isEffective: boolean | null;
	sourcePriority: number;
	protectedFields: string[];
	detail: string | null;
}

export interface InventoryWarning {
	client: InventoryClient | null;
	sourcePath: string;
	message: string;
}

export interface InventorySnapshot {
	records: InventoryRecord[];
	warnings: InventoryWarning[];
	capabilities: AdapterCapabilities[];
	scannedProjectCount: number;
}
