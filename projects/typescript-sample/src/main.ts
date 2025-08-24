interface User {
  id: number;
  name: string;
  email: string;
}

class UserService {
  private users: User[] = [];

  addUser(user: User): void {
    this.users.push(user);
    console.log(`Added user: ${user.name}`);
  }

  getUserById(id: number): User | undefined {
    return this.users.find(user => user.id === id);
  }

  getAllUsers(): User[] {
    return [...this.users];
  }
}

function main(): void {
  console.log("🚀 TypeScript Sample Project Starting...");
  
  const userService = new UserService();
  
  // Add some sample users
  userService.addUser({ id: 1, name: "Alice Johnson", email: "alice@example.com" });
  userService.addUser({ id: 2, name: "Bob Smith", email: "bob@example.com" });
  userService.addUser({ id: 3, name: "Carol Davis", email: "carol@example.com" });
  
  // Display all users
  console.log("\n📋 All Users:");
  const allUsers = userService.getAllUsers();
  allUsers.forEach(user => {
    console.log(`  - ${user.name} (${user.email})`);
  });
  
  // Find a specific user
  const user = userService.getUserById(2);
  if (user) {
    console.log(`\n🔍 Found user: ${user.name}`);
  }
  
  console.log("\n✅ TypeScript compilation successful!");
}

// Run the main function
main();
