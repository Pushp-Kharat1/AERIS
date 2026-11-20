import gymnasium as gym
from gymnasium import spaces
import numpy as np

class PortfolioMetaEnv(gym.Env):
    metadata = {"render_modes": ["human"]}

    def __init__(
        self,
        market_scenarios: np.ndarray,
        initial_balance: float = 100_000.0,
        max_leverage: float = 2.5,
        turnover_penalty_rate: float = 0.0005,
        drawdown_penalty_weight: float = 10.0
    ):
        super().__init__()
        self.market_scenarios = market_scenarios
        self.total_steps = len(market_scenarios)
        self.initial_balance = initial_balance
        self.max_leverage = max_leverage
        self.turnover_penalty_rate = turnover_penalty_rate
        self.drawdown_penalty_weight = drawdown_penalty_weight

        # 4 actions: unconstrained logits for [carry, maker, counter, cash]
        self.action_space = spaces.Box(
            low=-5.0,
            high=5.0,
            shape=(4,),
            dtype=np.float32
        )

        # 10 observations:
        # [vol_1h, vol_24h, funding_spread, delta_oi, spread_bps, drawdown, w_carry, w_maker, w_counter, w_cash]
        self.observation_space = spaces.Box(
            low=np.array([0.0, 0.0, -100.0, -50.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], dtype=np.float32),
            high=np.array([50.0, 50.0, 500.0, 50.0, 100.0, 1.0, 1.0, 1.0, 1.0, 1.0], dtype=np.float32),
            dtype=np.float32
        )

        self.reset()

    def reset(self, seed=None, options=None):
        super().reset(seed=seed)
        self.current_step = 0
        self.portfolio_equity = self.initial_balance
        self.high_water_mark = self.initial_balance
        self.current_drawdown = 0.0
        self.weights = np.array([0.25, 0.25, 0.0, 0.50], dtype=np.float32)
        return self._get_obs(), {}

    def _get_obs(self) -> np.ndarray:
        market_row = self.market_scenarios[self.current_step]
        # market_row: [vol_1h, vol_24h, funding_spread, delta_oi, spread_bps]
        obs = np.zeros(10, dtype=np.float32)
        obs[0:5] = market_row[0:5]
        obs[5] = self.current_drawdown
        obs[6:10] = self.weights
        return obs

    def _softmax(self, logits: np.ndarray) -> np.ndarray:
        exp_logits = np.exp(logits - np.max(logits))
        return exp_logits / (np.sum(exp_logits) + 1e-9)

    def _apply_deterministic_risk_shield(
        self,
        candidate_weights: np.ndarray,
        vol_1h: float,
        spread_bps: float
    ) -> np.ndarray:
        w = candidate_weights.copy()

        # Rule 1: High volatility or spread blowout -> completely purge Maker Arb
        if vol_1h > 2.5 or spread_bps > 3.0:
            w[1] = 0.0  # Zero maker

        # Rule 2: Minimum safe cash buffer during volatility expansion
        if vol_1h > 2.0 and w[3] < 0.20:
            w[3] = 0.20

        # Renormalize to simplex
        total_w = np.sum(w)
        if total_w > 0:
            w = w / total_w
        else:
            w = np.array([0.0, 0.0, 0.0, 1.0], dtype=np.float32)

        # Rule 3: Enforce hard gross leverage limit
        # Sleeve base leverages: Carry=2.0x, Maker=1.5x, Counter=2.5x, Cash=0.0x
        sleeve_leverages = np.array([2.0, 1.5, 2.5, 0.0], dtype=np.float32)
        gross_leverage = np.dot(w, sleeve_leverages)

        if gross_leverage > self.max_leverage:
            scale = self.max_leverage / gross_leverage
            # Scale down risk sleeves and transfer difference to cash
            risk_weights = w[:3] * scale
            w[:3] = risk_weights
            w[3] = 1.0 - np.sum(risk_weights)

        return np.clip(w, 0.0, 1.0)

    def step(self, action: np.ndarray):
        market_row = self.market_scenarios[self.current_step]
        vol_1h, vol_24h, funding_spread, delta_oi, spread_bps = market_row[0:5]

        # 1. Propose weights from policy action logits
        candidate_weights = self._softmax(action)

        # 2. Filter candidate weights through Deterministic Risk Shield
        target_weights = self._apply_deterministic_risk_shield(candidate_weights, vol_1h, spread_bps)

        # 3. Calculate turnover friction
        turnover = float(np.sum(np.abs(target_weights - self.weights)))
        # Friction: 5 bps taker fee on rebalanced volume
        turnover_cost = turnover * 0.0005 * self.portfolio_equity

        # 4. Compute sleeve returns over 15-minute interval (1/96 of a day)
        dt_day = 1.0 / 96.0

        # Carry Sleeve: Earns funding spread - basis drift drag
        annualized_spread_pct = funding_spread / 100.0
        r_carry = (annualized_spread_pct / 365.0) * dt_day * 2.0  # 2x leverage

        # Maker Arb Sleeve: Captures spread during quiet markets, suffers adverse selection during vol
        if vol_1h < 1.5 and spread_bps < 2.0:
            r_maker = 0.0003  # +3 bps net capture
        else:
            # Adverse selection loss when book is swept
            r_maker = -0.0015 * (vol_1h / 2.0)

        # Counter-Punch Sleeve:
        # If severe liquidation cascade (vol > 3.0 and delta_oi < -2.0):
        # 75% probability of successful wick absorption (+140 bps), 25% stopped out (-35 bps)
        r_counter = 0.0
        if vol_1h > 3.0 and delta_oi < -2.0:
            # Expected positive EV on wick exhaustion
            r_counter = 0.0105  # net return on active trade

        # Cash Sleeve: 0 return
        r_cash = 0.0

        # Weighted portfolio return
        sleeve_returns = np.array([r_carry, r_maker, r_counter, r_cash], dtype=np.float32)
        gross_pnl = self.portfolio_equity * float(np.dot(target_weights, sleeve_returns))
        net_pnl = gross_pnl - turnover_cost

        # Update equity and high water mark
        self.portfolio_equity += net_pnl
        if self.portfolio_equity > self.high_water_mark:
            self.high_water_mark = self.portfolio_equity
        self.current_drawdown = max(0.0, (self.high_water_mark - self.portfolio_equity) / (self.high_water_mark + 1e-9))
        self.weights = target_weights

        # Step Return
        step_return = net_pnl / (self.portfolio_equity - net_pnl + 1e-9)

        # 5. Differential Sortino / Downside Risk Penalized Reward
        downside_penalty = 2.0 * (min(0.0, step_return) ** 2)
        drawdown_excess = max(0.0, self.current_drawdown - 0.03)
        drawdown_penalty = self.drawdown_penalty_weight * (drawdown_excess ** 2)
        turnover_penalty = self.turnover_penalty_rate * turnover

        reward = float(step_return - downside_penalty - drawdown_penalty - turnover_penalty)

        self.current_step += 1
        terminated = self.current_step >= (self.total_steps - 1)
        truncated = self.portfolio_equity <= (self.initial_balance * 0.70)  # Liquidation termination

        info = {
            "portfolio_equity": self.portfolio_equity,
            "drawdown": self.current_drawdown,
            "net_pnl": net_pnl,
            "turnover": turnover,
            "weights": self.weights
        }

        return self._get_obs(), reward, terminated, truncated, info

