use crate::{ LoadBalancer,  Client, ServerGroup, HealthChecker };

pub fn load_configuration() -> Result<LoadBalancer, Box<dyn std::error::Error>>
{
    let mut lb = LoadBalancer::new()?;

    lb.clients.insert("first@first.com".into(), Client::new("first@first.com".into(), 0));
    lb.clients.insert("second@second.com".into(), Client::new("second@second.com".into(), 1));
    lb.clients.insert("third@third.com".into(), Client::new("third@third.com".into(), 2));
    lb.clients.insert("fourth@fourth.com".into(), Client::new("fourth@fourth.com".into(), 3));

    let mut sg0 = ServerGroup::new(0);

    sg0.server_addrs.insert(0, "tcp://127.0.0.1:2500".into());
    sg0.server_health.insert(0, HealthChecker::new(0, "tcp://127.0.0.1:2500".into()));
    sg0.server_addrs.insert(1, "tcp://127.0.0.1:2501".into());
    sg0.server_health.insert(1, HealthChecker::new(1, "tcp://127.0.0.1:2501".into()));
    sg0.server_addrs.insert(2, "tcp://127.0.0.1:2502".into());
    sg0.server_health.insert(2, HealthChecker::new(2, "tcp://127.0.0.1:2502".into()));

    lb.server_groups.insert(0, sg0);
    
    let mut sg1 = ServerGroup::new(1);

    sg1.server_addrs.insert(3, "tcp://127.0.0.1:2500".into());
    sg1.server_health.insert(3, HealthChecker::new(3, "tcp://127.0.0.1:2503".into()));
    sg1.server_addrs.insert(4, "tcp://127.0.0.1:2501".into());
    sg1.server_health.insert(4, HealthChecker::new(4, "tcp://127.0.0.1:2504".into()));
    sg1.server_addrs.insert(5, "tcp://127.0.0.1:2502".into());
    sg1.server_health.insert(5, HealthChecker::new(5, "tcp://127.0.0.1:2505".into()));

    lb.server_groups.insert(0, sg1);

    return Ok(lb);
}


