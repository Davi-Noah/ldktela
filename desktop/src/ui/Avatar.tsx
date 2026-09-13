interface AvatarProps {
  url: string | null;
  name: string;
  size?: number;
}

/** Dimensions are always explicit so a late-loading avatar never reflows a list. */
export function Avatar({ url, name, size = 24 }: AvatarProps) {
  const initial = name.slice(0, 1).toUpperCase();
  if (url === null) {
    return (
      <span
        aria-hidden="true"
        style={{ width: size, height: size, fontSize: size * 0.5 }}
        className="inline-flex shrink-0 items-center justify-center rounded-full bg-surface-3 text-text-muted"
      >
        {initial}
      </span>
    );
  }
  return (
    <img
      src={url}
      alt=""
      width={size}
      height={size}
      loading="lazy"
      className="shrink-0 rounded-full bg-surface-3"
    />
  );
}
