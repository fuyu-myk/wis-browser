import { useId, type ReactNode } from 'react'

interface SettingsGroupProps {
  /** Small heading over the panel, also the group's accessible name. */
  caption?: string
  children: ReactNode
}

export function SettingsGroup({ caption, children }: SettingsGroupProps) {
  const captionId = useId()
  return (
    <div className="kb-setgroup">
      {caption && (
        <div className="kb-setgroup-cap" id={captionId}>
          {caption}
        </div>
      )}
      <div className="kb-setgroup-box" role="group" aria-labelledby={caption ? captionId : undefined}>
        {children}
      </div>
    </div>
  )
}
