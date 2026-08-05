import type { HTMLAttributes, ReactNode } from 'react';
import { cx } from './classnames';

export interface CardShellProps extends Omit<HTMLAttributes<HTMLElement>, 'title'> {
  title?: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  footer?: ReactNode;
  children?: ReactNode;
}

export function CardShell({
  title,
  description,
  actions,
  footer,
  children,
  className,
  ...sectionProps
}: CardShellProps) {
  return (
    <section {...sectionProps} className={cx('lingxi-plugin-card', className)}>
      {(title || description || actions) && (
        <header className="lingxi-plugin-card-header">
          <div className="lingxi-plugin-card-heading">
            {title && <h2 className="lingxi-plugin-card-title">{title}</h2>}
            {description && <p className="lingxi-plugin-card-description">{description}</p>}
          </div>
          {actions && <div className="lingxi-plugin-card-actions">{actions}</div>}
        </header>
      )}
      <div className="lingxi-plugin-card-body">{children}</div>
      {footer && <footer className="lingxi-plugin-card-footer">{footer}</footer>}
    </section>
  );
}

export interface SettingRowProps extends HTMLAttributes<HTMLDivElement> {
  label: ReactNode;
  hint?: ReactNode;
  control: ReactNode;
  layout?: 'inline' | 'stacked';
}

export function SettingRow({
  label,
  hint,
  control,
  layout = 'inline',
  className,
  ...rowProps
}: SettingRowProps) {
  return (
    <div
      {...rowProps}
      className={cx(
        'lingxi-plugin-setting-row',
        layout === 'stacked' ? 'lingxi-plugin-setting-row-stacked' : 'lingxi-plugin-setting-row-inline',
        className,
      )}
    >
      <div className="lingxi-plugin-setting-text">
        <div className="lingxi-plugin-setting-label">{label}</div>
        {hint && <div className="lingxi-plugin-setting-hint">{hint}</div>}
      </div>
      <div className="lingxi-plugin-setting-control">{control}</div>
    </div>
  );
}

export interface EmptyStateProps extends Omit<HTMLAttributes<HTMLDivElement>, 'title'> {
  icon?: ReactNode;
  title: ReactNode;
  description?: ReactNode;
  action?: ReactNode;
}

export function EmptyState({ icon, title, description, action, className, ...rootProps }: EmptyStateProps) {
  return (
    <div {...rootProps} className={cx('lingxi-plugin-empty', className)}>
      {icon && <div className="lingxi-plugin-empty-icon">{icon}</div>}
      <div className="lingxi-plugin-empty-title">{title}</div>
      {description && <div className="lingxi-plugin-empty-description">{description}</div>}
      {action && <div className="lingxi-plugin-empty-action">{action}</div>}
    </div>
  );
}

export interface ListItem {
  id: string;
  title: ReactNode;
  description?: ReactNode;
  meta?: ReactNode;
  icon?: ReactNode;
  action?: ReactNode;
}

export interface ListProps extends HTMLAttributes<HTMLUListElement> {
  items: ListItem[];
}

export function List({ items, className, ...listProps }: ListProps) {
  return (
    <ul {...listProps} className={cx('lingxi-plugin-list', className)}>
      {items.map((item) => (
        <li key={item.id} className="lingxi-plugin-list-item">
          {item.icon && <div className="lingxi-plugin-list-icon">{item.icon}</div>}
          <div className="lingxi-plugin-list-main">
            <div className="lingxi-plugin-list-line">
              <span className="lingxi-plugin-list-title">{item.title}</span>
              {item.meta && <span className="lingxi-plugin-list-meta">{item.meta}</span>}
            </div>
            {item.description && <div className="lingxi-plugin-list-description">{item.description}</div>}
          </div>
          {item.action && <div className="lingxi-plugin-list-action">{item.action}</div>}
        </li>
      ))}
    </ul>
  );
}
