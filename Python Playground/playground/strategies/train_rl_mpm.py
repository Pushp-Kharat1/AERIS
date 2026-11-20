import numpy as np
import torch
import torch.nn as nn
import torch.optim as optim
from torch.distributions.normal import Normal
from rl_meta_env import PortfolioMetaEnv

class ActorCritic(nn.Module):
    def __init__(self, obs_dim: int = 10, action_dim: int = 4):
        super().__init__()
        self.shared = nn.Sequential(
            nn.Linear(obs_dim, 64),
            nn.Tanh(),
            nn.Linear(64, 64),
            nn.Tanh()
        )
        self.actor_mean = nn.Linear(64, action_dim)
        self.actor_logstd = nn.Parameter(torch.zeros(action_dim))
        self.critic = nn.Linear(64, 1)

    def forward(self, x):
        features = self.shared(x)
        action_mean = self.actor_mean(features)
        action_std = torch.exp(self.actor_logstd.expand_as(action_mean))
        value = self.critic(features)
        return action_mean, action_std, value

def generate_market_scenarios(n_steps: int = 2000) -> np.ndarray:
    np.random.seed(42)
    scenarios = np.zeros((n_steps, 5), dtype=np.float32)
    # [vol_1h, vol_24h, funding_spread, delta_oi, spread_bps]

    # Mix regimes: 60% Quiet Carry, 25% Normal Vol, 10% Vol Expansion, 5% Violent Liquidation Crash
    regimes = np.random.choice([0, 1, 2, 3], size=n_steps, p=[0.60, 0.25, 0.10, 0.05])

    for i in range(n_steps):
        r = regimes[i]
        if r == 0:  # Quiet Carry
            scenarios[i] = [
                float(np.random.uniform(0.4, 1.2)),
                float(np.random.uniform(0.8, 1.5)),
                float(np.random.uniform(18.0, 26.0)),
                float(np.random.uniform(-0.5, 0.5)),
                float(np.random.uniform(0.8, 1.5))
            ]
        elif r == 1:  # Normal Volatility
            scenarios[i] = [
                float(np.random.uniform(1.2, 2.4)),
                float(np.random.uniform(1.5, 3.0)),
                float(np.random.uniform(10.0, 18.0)),
                float(np.random.uniform(-1.5, 1.5)),
                float(np.random.uniform(1.2, 2.8))
            ]
        elif r == 2:  # Volatility Expansion (Fakeouts)
            scenarios[i] = [
                float(np.random.uniform(2.6, 4.5)),
                float(np.random.uniform(3.0, 5.5)),
                float(np.random.uniform(5.0, 14.0)),
                float(np.random.uniform(-3.0, 3.0)),
                float(np.random.uniform(3.5, 8.0))
            ]
        else:  # Violent Liquidation Crash (Aug 5 style)
            scenarios[i] = [
                float(np.random.uniform(6.0, 18.0)),
                float(np.random.uniform(8.0, 22.0)),
                float(np.random.uniform(-15.0, 10.0)),
                float(np.random.uniform(-8.0, -2.5)),
                float(np.random.uniform(10.0, 35.0))
            ]
    return scenarios

def train_ppo_mpm(epochs: int = 50, steps_per_epoch: int = 500):
    scenarios = generate_market_scenarios(n_steps=steps_per_epoch * 2)
    env = PortfolioMetaEnv(market_scenarios=scenarios)
    model = ActorCritic(obs_dim=10, action_dim=4)
    optimizer = optim.Adam(model.parameters(), lr=3e-4)

    gamma = 0.99
    clip_eps = 0.2

    print("[*] Training AERIS PPO Master Parent Model...")
    for epoch in range(epochs):
        obs, _ = env.reset()
        obs_buf, act_buf, rew_buf, val_buf, logp_buf = [], [], [], [], []

        for _ in range(steps_per_epoch):
            obs_t = torch.as_tensor(obs, dtype=torch.float32)
            with torch.no_grad():
                mean, std, val = model(obs_t)
                dist = Normal(mean, std)
                action = dist.sample()
                logp = dist.log_prob(action).sum()

            next_obs, rew, term, trunc, _ = env.step(action.numpy())

            obs_buf.append(obs)
            act_buf.append(action.numpy())
            rew_buf.append(rew)
            val_buf.append(val.item())
            logp_buf.append(logp.item())

            obs = next_obs
            if term or trunc:
                obs, _ = env.reset()

        # Compute discounted returns & advantages
        returns = []
        discounted_r = 0
        for r in reversed(rew_buf):
            discounted_r = r + gamma * discounted_r
            returns.insert(0, discounted_r)

        obs_t = torch.as_tensor(np.array(obs_buf), dtype=torch.float32)
        act_t = torch.as_tensor(np.array(act_buf), dtype=torch.float32)
        old_logp_t = torch.as_tensor(np.array(logp_buf), dtype=torch.float32)
        ret_t = torch.as_tensor(returns, dtype=torch.float32)
        val_t = torch.as_tensor(val_buf, dtype=torch.float32)
        adv_t = ret_t - val_t
        adv_t = (adv_t - adv_t.mean()) / (adv_t.std() + 1e-8)

        # PPO Update
        for _ in range(4):
            mean, std, val = model(obs_t)
            dist = Normal(mean, std)
            new_logp = dist.log_prob(act_t).sum(dim=-1)

            ratio = torch.exp(new_logp - old_logp_t)
            surr1 = ratio * adv_t
            surr2 = torch.clamp(ratio, 1.0 - clip_eps, 1.0 + clip_eps) * adv_t
            actor_loss = -torch.min(surr1, surr2).mean()
            critic_loss = nn.functional.mse_loss(val.squeeze(-1), ret_t)

            loss = actor_loss + 0.5 * critic_loss
            optimizer.zero_grad()
            loss.backward()
            optimizer.step()

        if (epoch + 1) % 10 == 0 or epoch == 0:
            print(f"Epoch {epoch+1:02d}/{epochs:02d} | Loss: {loss.item():.4f} | Final Equity: ${env.portfolio_equity:,.2f} | Max DD: {env.current_drawdown*100:.2f}%")

    from pathlib import Path
    save_dir = Path(__file__).resolve().parent.parent / "data"
    save_dir.mkdir(parents=True, exist_ok=True)
    model_path = save_dir / "rl_mpm_policy.pt"
    torch.save(model.state_dict(), model_path)
    print(f"[+] Model saved to {model_path}")
    return model

def benchmark_models(trained_model: ActorCritic):
    test_scenarios = generate_market_scenarios(n_steps=1000)

    # 1. Benchmark: Pure Carry (100% Carry, 0% Maker, 0% Counter, 0% Cash)
    env_carry = PortfolioMetaEnv(market_scenarios=test_scenarios)
    obs_carry, _ = env_carry.reset()
    for _ in range(len(test_scenarios) - 1):
        # Action corresponding to max carry
        action = np.array([5.0, -5.0, -5.0, -5.0], dtype=np.float32)
        obs_carry, _, term, trunc, _ = env_carry.step(action)
        if term or trunc:
            break

    # 2. Benchmark: Heuristic Rule-Based MPM
    env_rule = PortfolioMetaEnv(market_scenarios=test_scenarios)
    obs_rule, _ = env_rule.reset()
    for _ in range(len(test_scenarios) - 1):
        vol_1h = obs_rule[0]
        # Heuristic mapping:
        if vol_1h > 3.5:
            # Crash/vol expansion -> pull maker, high counter & cash
            action = np.array([0.0, -5.0, 3.0, 2.0], dtype=np.float32)
        elif vol_1h > 1.2:
            # Normal vol -> balanced
            action = np.array([2.0, 1.0, 0.0, 1.0], dtype=np.float32)
        else:
            # Quiet carry -> high carry
            action = np.array([4.0, 2.0, -5.0, 0.0], dtype=np.float32)

        obs_rule, _, term, trunc, _ = env_rule.step(action)
        if term or trunc:
            break

    # 3. Benchmark: Trained RL Policy
    env_rl = PortfolioMetaEnv(market_scenarios=test_scenarios)
    obs_rl, _ = env_rl.reset()
    for _ in range(len(test_scenarios) - 1):
        obs_t = torch.as_tensor(obs_rl, dtype=torch.float32)
        with torch.no_grad():
            mean, _, _ = trained_model(obs_t)
            action = mean.numpy()
        obs_rl, _, term, trunc, _ = env_rl.step(action)
        if term or trunc:
            break

    print("\n" + "=" * 65)
    print("EMPIRICAL BENCHMARK: PURE CARRY vs HEURISTIC MPM vs RL AGENT")
    print("=" * 65)
    print(f"{'Strategy':<25} | {'Final Equity':<12} | {'Net Return':<10} | {'Max DD':<8}")
    print("-" * 65)
    print(f"{'1. Pure 100% Carry':<25} | ${env_carry.portfolio_equity:<11,.2f} | {((env_carry.portfolio_equity/100000.0)-1)*100:>8.2f}% | {env_carry.current_drawdown*100:>6.2f}%")
    print(f"{'2. Heuristic Rule MPM':<25} | ${env_rule.portfolio_equity:<11,.2f} | {((env_rule.portfolio_equity/100000.0)-1)*100:>8.2f}% | {env_rule.current_drawdown*100:>6.2f}%")
    print(f"{'3. Trained RL Agent':<25} | ${env_rl.portfolio_equity:<11,.2f} | {((env_rl.portfolio_equity/100000.0)-1)*100:>8.2f}% | {env_rl.current_drawdown*100:>6.2f}%")
    print("=" * 65)

if __name__ == "__main__":
    trained_model = train_ppo_mpm(epochs=50, steps_per_epoch=500)
    benchmark_models(trained_model)
