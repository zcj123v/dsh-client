import { displayConnection, type ConnectionRecord } from './connection-model'
import type { TabsSnapshot } from './native-tabs'

export const TAB_BAR_HEIGHT_CSS_PX = 56

interface TabBarProps {
  snapshot: TabsSnapshot | null
  onSelect: (id: string) => void
  onManager: () => void
  onClose: (id: string, name: string) => void
  onNew: () => void
}

export function TabBar({ snapshot, onSelect, onManager, onClose, onNew }: TabBarProps) {
  const activeId = snapshot?.activeId ?? null
  return (
    <div className="native-tabbar" role="tablist" aria-label="节点标签" data-testid="native-tabbar">
      <button type="button" role="tab" aria-selected={activeId === null} className={`native-tab manager-tab${activeId === null ? ' selected' : ''}`} onClick={onManager}>
        <span className="native-tab-icon">⌂</span>
        <span className="native-tab-name">节点管理</span>
      </button>
      {(snapshot?.tabs ?? []).map(tab => (
        <div className={`native-tab-item${activeId === tab.id ? ' selected' : ''}`} key={tab.id}>
          <button type="button" role="tab" aria-selected={activeId === tab.id} className={`native-tab state-${tab.state}`} onClick={() => onSelect(tab.id)}>
            <span className="native-tab-name">{tab.name || tab.id}</span>
            {tab.state === 'opening' && <span className="native-tab-state">连接中</span>}
            {tab.state === 'failed' && <span className="native-tab-state">失败</span>}
          </button>
          <button type="button" className="native-tab-close" aria-label={`关闭标签 ${tab.name || tab.id}`} title="关闭标签" onClick={() => onClose(tab.id, tab.name)}>×</button>
        </div>
      ))}
      <button type="button" className="native-tab-new" aria-label="新建连接标签" title="新建连接标签" onClick={onNew}>＋</button>
    </div>
  )
}

interface TabChooserProps {
  connections: ConnectionRecord[]
  openTabIds: Set<string>
  onChoose: (record: ConnectionRecord) => void
  onNewNode: () => void
  onDismiss: () => void
}

export function TabChooser({ connections, openTabIds, onChoose, onNewNode, onDismiss }: TabChooserProps) {
  return (
    <div className="tab-chooser-backdrop" onClick={onDismiss}>
      <div className="tab-chooser" role="dialog" aria-label="选择节点" onClick={event => event.stopPropagation()}>
        <p className="tab-chooser-title">打开节点标签</p>
        <p className="tab-chooser-hint">选择已保存节点打开标签；已有标签的节点会直接切换，不重新连接。</p>
        <div className="tab-chooser-list">
          {connections.length === 0
            ? <p className="tab-chooser-empty">还没有已保存节点，先新增一个。</p>
            : connections.map(record => (
              <button type="button" key={record.id} className="tab-chooser-node" onClick={() => onChoose(record)}>
                <strong>{record.name}</strong>
                <span>{openTabIds.has(record.id) ? '已打开 · 点击切换' : displayConnection(record)}</span>
              </button>
            ))}
        </div>
        <div className="tab-chooser-actions">
          <button type="button" className="secondary-button tab-chooser-new" onClick={onNewNode}>新增节点</button>
          <button type="button" className="tab-chooser-cancel" onClick={onDismiss}>取消</button>
        </div>
      </div>
    </div>
  )
}
