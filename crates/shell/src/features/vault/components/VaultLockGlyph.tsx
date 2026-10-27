import { cx } from '@/lib/cx'

export function VaultLockGlyph({ open }: { open: boolean }) {
  return (
    <span className={cx('kb-vault-lock-icon', 'kb-vault-lockglyph', { 'is-open': open })} aria-hidden>
      <svg
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <path className="kb-vault-shackle" d="M8 11V7a4 4 0 0 1 8 0v4" />
        <rect x="5" y="11" width="14" height="10" rx="2" />
      </svg>
    </span>
  )
}
