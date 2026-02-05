import { accessSync, constants, existsSync } from 'fs';

function envTruthy(key: string): boolean {
  const value = process.env[key];
  if (!value) return false;
  switch (value.toLowerCase()) {
    case '1':
    case 'true':
    case 'yes':
    case 'on':
      return true;
    default:
      return false;
  }
}

function kvmAvailable(): boolean {
  if (!existsSync('/dev/kvm')) return false;
  try {
    accessSync('/dev/kvm', constants.R_OK | constants.W_OK);
    return true;
  } catch {
    return false;
  }
}

async function firecrackerAvailable(): Promise<boolean> {
  try {
    const proc = Bun.spawn(['firecracker', '--version'], {
      stdout: 'pipe',
      stderr: 'pipe',
    });
    await proc.exited;
    return proc.exitCode === 0;
  } catch {
    return false;
  }
}

export async function shouldUseMockRunner(): Promise<boolean> {
  if (envTruthy('GRAPHENE_FORCE_MOCK_RUNNER')) {
    return true;
  }

  if (process.platform !== 'linux') {
    return true;
  }

  if (!kvmAvailable()) {
    return true;
  }

  const firecrackerOk = await firecrackerAvailable();
  return !firecrackerOk;
}
