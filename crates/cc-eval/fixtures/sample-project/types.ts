export interface BaseEntity {
  id: string;
  createdAt: Date;
}

export interface UserProfile extends BaseEntity {
  name: string;
  email: string;
}

export type UserRole = 'admin' | 'user' | 'guest';

export class UserService {
  getProfile(id: string): UserProfile {
    return {} as UserProfile;
  }
}
