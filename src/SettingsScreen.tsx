import type { ReactNode } from "react";
import { FieldIntegration } from "./FieldIntegration";
import { Icon } from "./icons";
import { McpConnections } from "./McpConnections";
import { StorageSettings } from "./Features";
import type { WorkspaceStore } from "./useWorkspace";

type SettingsScreenProps = {
  store: WorkspaceStore;
  workspaceId: string;
  workspaceName: string;
  onSelectItem: (id: string) => void;
};

function SettingsSection({
  id,
  icon,
  title,
  description,
  children,
}: {
  id: string;
  icon: string;
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <section className="panel settings-section" id={id}>
      <header className="settings-section-header">
        <span className="settings-section-icon">
          <Icon name={icon} size={22} weight="duotone" />
        </span>
        <div>
          <h2>{title}</h2>
          <p>{description}</p>
        </div>
      </header>
      {children}
    </section>
  );
}

export function SettingsScreen({
  store,
  workspaceId,
  workspaceName,
  onSelectItem,
}: SettingsScreenProps) {
  return (
    <div className="page-content settings-page">
      <section className="panel settings-overview">
        <div className="settings-overview-copy">
          <span className="eyebrow">{workspaceName}の設定</span>
          <h2>必要な設定を、場所ごとに。</h2>
          <p>
            表示の好み、外部サービスとの連携、AIクライアントの権限、
            バックアップをそれぞれの場所で確認・変更できます。
          </p>
        </div>
        <nav className="settings-index" aria-label="設定の目次">
          <a href="#settings-display">
            <Icon name="settings" size={20} weight="duotone" />
            <span>
              <strong>表示とデータ</strong>
              <small>表示・日付・バックアップ</small>
            </span>
            <Icon name="right" size={14} />
          </a>
          <a href="#settings-field">
            <Icon name="link" size={20} weight="duotone" />
            <span>
              <strong>外部サービス</strong>
              <small>Tachyon・Field連携</small>
            </span>
            <Icon name="right" size={14} />
          </a>
          <a href="#settings-ai">
            <Icon name="sparkle" size={20} weight="duotone" />
            <span>
              <strong>AIとの接続</strong>
              <small>接続と許可する操作</small>
            </span>
            <Icon name="right" size={14} />
          </a>
        </nav>
      </section>

      <div className="settings-sections">
        <SettingsSection
          id="settings-display"
          icon="settings"
          title="表示とデータ"
          description="PathBaseの表示方法、日付の基準、データの持ち出しを管理します。"
        >
          <StorageSettings
            key={workspaceId}
            store={store}
            workspaceId={workspaceId}
            onSelect={onSelectItem}
          />
        </SettingsSection>

        <SettingsSection
          id="settings-field"
          icon="link"
          title="外部サービス"
          description="Tachyonの認証状態を確認し、Fieldの営業タスクや成果指標を参照します。"
        >
          <FieldIntegration store={store} workspaceId={workspaceId} />
        </SettingsSection>

        <SettingsSection
          id="settings-ai"
          icon="sparkle"
          title="AIとの接続"
          description="接続中のAIクライアントと、委ねる操作の範囲を管理します。"
        >
          <McpConnections store={store} />
        </SettingsSection>
      </div>
    </div>
  );
}
