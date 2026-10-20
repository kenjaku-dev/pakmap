import { NAV_ITEMS } from "@/lib/site";

type NavLinksProps = {
  activeHref?: string;
  onNavigate?: () => void;
  className?: string;
  linkClassName?: string;
};

function isActive(href: string, activeHref?: string) {
  return activeHref === href;
}

export function NavLinks({
  activeHref,
  onNavigate,
  className = "",
  linkClassName = "",
}: NavLinksProps) {
  return (
    <ul className={`flex list-none flex-col gap-1 sm:flex-row sm:items-center sm:gap-1 ${className}`}>
      {NAV_ITEMS.map((item) => {
        const active = isActive(item.href, activeHref);
        return (
          <li key={item.href}>
            <a
              href={item.href}
              onClick={onNavigate}
              aria-current={active ? "location" : undefined}
              className={`inline-flex min-h-11 items-center rounded-md px-3 text-sm transition-colors duration-200 ${linkClassName} ${
                active
                  ? "text-foreground underline decoration-accent decoration-2 underline-offset-4"
                  : "text-muted-foreground hover:text-foreground"
              }`}
            >
              {item.label}
            </a>
          </li>
        );
      })}
    </ul>
  );
}
