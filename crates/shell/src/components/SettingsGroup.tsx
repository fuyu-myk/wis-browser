import { Children, useId, type ReactNode } from 'react'

interface SettingsGroupProps {
  /** Small heading over the panel, also the group's accessible name. */
  caption?: string
  children: ReactNode
}

export function SettingsGroup({ caption, children }: SettingsGroupProps) {
  const captionId = useId()
  if (Children.toArray(children).length === 0) return null
  return (
    <div className="kb-setgroup">
      {caption && (
        <h3 className="kb-setgroup-cap" id={captionId}>
          {caption}
        </h3>
      )}
      <div className="kb-setgroup-box" role="group" aria-labelledby={caption ? captionId : undefined}>
        {children}
      </div>
    </div>
  )
}
