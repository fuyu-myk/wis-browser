import { useId, type ReactNode } from 'react'

import { useRevealWhole } from '@/features/vault/hooks/useRevealWhole'
import type { VaultField } from '@/features/vault/lib/vaultSections'
import { ChevronRightIcon } from '@/lib/icons'

interface VaultRowProps {
  title: string
  detail?: string
  /** Shown beside the title while closed, such as a breach badge. */
  badge?: ReactNode
  open: boolean
  onToggle: () => void
  /** The open row's content: its fields and actions, or an edit form. */
  children: ReactNode
}

/** A saved password, address or card that opens in place on click. */
export function VaultRow({ title, detail, badge, open, onToggle, children }: VaultRowProps) {
  const bodyId = useId()
  const reveal = useRevealWhole<HTMLLIElement>()
  return (
    <li className="kb-vault-row" ref={open ? reveal : undefined}>
      <button
        type="button"
        className="kb-vault-head"
        aria-expanded={open}
        aria-controls={open ? bodyId : undefined}
        onClick={onToggle}
      >
        <span className="kb-vault-id">
          <span className="kb-vault-host">{title}</span>
          {detail && <span className="kb-vault-user">{detail}</span>}
        </span>
        {badge}
        <ChevronRightIcon className="kb-vault-chevron" />
      </button>
      {open && (
        <div className="kb-vault-body" id={bodyId}>
          {children}
        </div>
      )}
    </li>
  )
}

export function VaultFields({ fields }: { fields: ReadonlyArray<VaultField> }) {
  return (
    <dl className="kb-vault-fields">
      {fields.map(({ label, value }) => (
        <div key={label}>
          <dt>{label}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  )
}

interface VaultActionsProps {
  children?: ReactNode
  removeLabel: string
  removing?: boolean
  onRemove: () => void
}

export function VaultActions({ children, removeLabel, removing, onRemove }: VaultActionsProps) {
  return (
    <div className="kb-vault-acts">
      {children}
      <button
        type="button"
        className="kb-vault-action is-danger"
        aria-label={removeLabel}
        disabled={removing}
        onClick={onRemove}
      >
        Remove
      </button>
    </div>
  )
}

export function Revealed({ children }: { children: ReactNode }) {
  const reveal = useRevealWhole<HTMLDivElement>()
  return <div ref={reveal}>{children}</div>
}
