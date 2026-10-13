#include "setup.hpp"

void main_setup() {
	const uint Nx=176u, Ny=176u, Nz=251u;
	const float u=0.04f, side=25.0f, re=65963.0f;
	const ulong settle=7551ull, average=12560ull, every=50ull;
	LBM lbm(Nx, Ny, Nz, u*side/re);
	parallel_for(lbm.get_N(), [&](ulong n) { uint x=0u, y=0u, z=0u; lbm.coordinates(n, x, y, z);
		if(x>=76u&&x<=100u&&y>=76u&&y<=100u&&z>=100u&&z<=124u) lbm.flags[n] = TYPE_S;
		else lbm.u.z[n] = u;
		if(x==0u||x==Nx-1u||y==0u||y==Ny-1u||z==0u||z==Nz-1u) lbm.flags[n] = TYPE_E;
	});
	lbm.run(0u);
	const auto start = std::chrono::steady_clock::now();
	lbm.run(settle);
	double sum=0.0; ulong samples=0ull;
	for(ulong t=0ull; t<average; t+=every) {
		lbm.run(every);
		lbm.update_force_field();
		sum += (double)lbm.object_force(TYPE_S).z;
		samples++;
	}
	const double seconds = std::chrono::duration<double>(std::chrono::steady_clock::now()-start).count();
	const double cd = sum/(double)samples/(0.5*u*u*side*side);
	print_info("CUBE cd "+to_string(cd, 4u)+" drag_N "+to_string(cd*0.5*1.204*100.0*0.01, 4u)+" seconds "+to_string(seconds, 1u));
}
