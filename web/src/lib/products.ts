export type Product = {
  name: string;
  shortName: string;
  role: string;
  href: string;
  accent: string;
  active?: boolean;
  group: 'pillar' | 'tool';
};

export const products: Product[] = [
  {
    name: 'U.N. Avatar',
    shortName: 'Avatar',
    role: 'Renderer',
    href: 'https://usagi.github.io/un-avatar/',
    accent: '#ff6f4d',
    active: true,
    group: 'pillar'
  },
  {
    name: 'U.N. Motion',
    shortName: 'Motion',
    role: 'Tracking',
    href: 'https://github.com/usagi/un-motion',
    accent: '#2f7dff',
    group: 'pillar'
  },
  {
    name: 'U.N. Virtual Avatar Connect',
    shortName: 'Connect',
    role: 'Hub',
    href: 'https://github.com/usagi/un-virtual-avatar-connect',
    accent: '#38bdf8',
    group: 'pillar'
  },
  {
    name: 'U.N. VRC PerfectSync',
    shortName: 'PerfectSync',
    role: 'Unity Tool',
    href: 'https://usagi.github.io/un-vrc-perfectsync/',
    accent: '#67d9b3',
    group: 'pillar'
  },
  {
    name: 'U.N. Virtual Eye Tracker',
    shortName: 'VET',
    role: 'Tool',
    href: 'https://github.com/usagi/un-virtual-eye-tracker',
    accent: '#16c4bb',
    group: 'tool'
  }
];
