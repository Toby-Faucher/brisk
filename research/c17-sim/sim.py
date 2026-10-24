import random, math
FPS=60; DT=1/FPS; H=5  # 5 frames ~83 ms hidden
KN=[0,.1,.2,.35,.5,.7,.9]; PXDEG=1920/90
def hing(u): s=1 if u>=0 else -1; return [s*max(abs(u)-c,0) for c in KN]
def traj(T,rng):
    u=[0.0]*T; k=0; 
    while k<T:
        r=rng.random(); n=int(rng.uniform(.3,1.5)*FPS)
        if r<.5:   # smooth pan
            a=rng.uniform(-.8,.8)
            for i in range(n):
                if k+i<T: u[k+i]=a*math.sin(math.pi*i/n)
        elif r<.8: # flick: near full deflection, short
            a=rng.choice([-1,1])*rng.uniform(.85,1); m=int(rng.uniform(.1,.25)*FPS)
            for i in range(m):
                if k+i<T: u[k+i]=a
            n=m
        else: pass # idle
        k+=n+int(rng.uniform(.1,.6)*FPS)
    return u
def plant(u,aim=False,ads=False,rng=None):
    # deadzone .12, expo curve, G=300 deg/s, lag tau=50 ms; unobserved extras
    G=300;dz=.12;tau=.05;a=math.exp(-DT/tau);w=[0.0]*len(u);s=0.0
    aa=0;gs=1.0
    for k,x in enumerate(u):
        if k%(FPS*4)==0: aa=1 if rng.random()<.4 else 0; gs=.45 if (ads and rng.random()<.4) else 1.0
        d=0 if abs(x)<dz else math.copysign((abs(x)-dz)/(1-dz),x)
        t=G*gs*(.6*d+.4*d**3)
        if aim and aa: t*=0.55   # aim-assist friction, unobservable from input
        s=a*s+(1-a)*t; w[k]=s*DT   # deg/frame
    return w
class RLS:
    def __init__(s,n,lam=.998): s.n=n;s.th=[0.0]*n;s.P=[[1e3 if i==j else 0 for j in range(n)] for i in range(n)];s.lam=lam
    def upd(s,x,y,clip=None):
        n=s.n;P=s.P;Px=[sum(P[i][j]*x[j] for j in range(n)) for i in range(n)]
        den=s.lam+sum(x[i]*Px[i] for i in range(n)); e=y-sum(s.th[i]*x[i] for i in range(n))
        if clip is not None and abs(e)>clip: return e   # robust: reject gross outliers
        for i in range(n): s.th[i]+=Px[i]*e/den
        for i in range(n):
            for j in range(n): P[i][j]=(P[i][j]-Px[i]*Px[j]/den)/s.lam
        return e
def run(seed,T=120*FPS,noise=.05,outl=0.0,aim=False,ads=False,robust=True,defgain=1.6):
    rng=random.Random(seed);u=traj(T,rng);w=plant(u,aim,ads,rng)
    m=[(w[k-H] if k>=H else 0)+rng.gauss(0,noise)+(rng.gauss(0,.6) if rng.random()<outl else 0) for k in range(T)]  # frame k shows game state k-H measured
    # measured w[k] reflects u[k-H]
    # lag search at 3 s using |u| vs |m|
    ts=3*FPS; best=max(range(0,13),key=lambda L:sum(abs(u[k-L])*abs(m[k]) for k in range(20,ts)))
    L=best; r=RLS(1+len(KN)); res=[]; e0=[];e1=[];e2=[]; rw=[0.0]*T
    for k in range(T):
        if k>=ts:
            x=[m[k-1]]+hing(u[k-L]); r.upd(x,m[k],clip=(.35 if robust else None))
        if k<ts+FPS or k<L+2: pred=0.0
        else:
            s=m[k]; pred=0.0; th=r.th   # roll model forward H steps over unsent-to-video inputs
            for i in range(1,H+1):
                s=th[0]*s+sum(th[1+j]*v for j,v in enumerate(hing(u[k-H+i])))
                pred+=s
        true=sum(w[k-H+1:k+1])
                # naive: raw linear gain defgain*G, no lag, no deadzone
        naive=sum(defgain*300*DT*u[k-H+i] for i in range(1,H+1))
        res.append((k,true,pred,naive))
    return res
def rms(v): return math.sqrt(sum(x*x for x in v)/max(1,len(v)))
def summarize(name,**kw):
    out=[]
    for sd in range(4):
        R=run(sd,**kw); late=[x for x in R if x[0]>=20*FPS]
        none=rms([t for _,t,_,_ in late]);warp=rms([t-p for _,t,p,_ in late]);nv=rms([t-n for _,t,_,n in late])
        fl=[abs(t-p) for _,t,p,_ in late if abs(t)>2.0]; p95=sorted([abs(t-p) for _,t,p,_ in late])[int(.95*len(late))]
        # convergence: first time 2s rolling rms err < 1.5*final
        errs=[t-p for _,t,p,_ in R]; fin=rms([t-p for k,t,p,_ in R if k>=20*FPS]);conv=None
        for i in range(0,len(errs)-2*FPS,FPS//2):
            if rms(errs[i:i+2*FPS])<=1.5*fin+1e-9 and R[i][0]>=3*FPS: conv=R[i][0]/FPS;break
        out.append((none,nv,warp,p95,conv))
    av=lambda i:sum(o[i] for o in out)/len(out)
    cs=[o[4] for o in out if o[4]]
    print(f"{name:26s} none {av(0):.2f}  naive {av(1):.2f}  RLS {av(2):.2f} deg RMS ({av(2)*PXDEG:.0f}px)  p95 {av(3):.2f}deg  conv {sum(cs)/len(cs) if cs else float('nan'):.1f}s")
if __name__=="__main__":
    summarize("clean")
    summarize("outl 5% N(0,.6deg)",outl=.05,robust=True)
    summarize("outl 5% no-robust",outl=.05,robust=False)
    summarize("outl 20% robust",outl=.20,robust=True)
    summarize("noise 0.2 deg/frame",noise=.2)
    summarize("aim-assist",aim=True)
    summarize("aim+outl5%",aim=True,outl=.05)
    summarize("ADS gain switch",ads=True)
