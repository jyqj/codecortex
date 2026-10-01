#include <libproc.h>
#include <sys/proc_info.h>
#include <sys/resource.h>
#include <mach/mach_time.h>
#include <stdint.h>
#include <inttypes.h>
#include <stdio.h>
#include <unistd.h>
#include <time.h>
#include <stdlib.h>
static uint64_t ru_us(struct timeval x){return (uint64_t)x.tv_sec*1000000+x.tv_usec;}
static uint64_t wall_ns(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return (uint64_t)t.tv_sec*1000000000+t.tv_nsec;}
static int snapshot(struct proc_taskinfo *p){return proc_pidinfo(getpid(),PROC_PIDTASKINFO,0,p,sizeof(*p));}
int main(void){mach_timebase_info_data_t tb;mach_timebase_info(&tb);printf("{\"timebase_numer\":%u,\"timebase_denom\":%u,\"pid\":%d,\"samples\":[",tb.numer,tb.denom,getpid());volatile uint64_t sink=1;
 for(int round=0;round<5;round++){struct proc_taskinfo a={0},b={0};struct rusage ra={0},rb={0};uint64_t wa=wall_ns();getrusage(RUSAGE_SELF,&ra);if(snapshot(&a)!=sizeof(a))return 2;uint64_t target=wa+300000000;
 while(wall_ns()<target){for(int j=0;j<5000;j++)sink=sink*2862933555777941757ULL+3037000493ULL;}
 if(snapshot(&b)!=sizeof(b))return 3;getrusage(RUSAGE_SELF,&rb);uint64_t wb=wall_ns();uint64_t pu=b.pti_total_user-a.pti_total_user,ps=b.pti_total_system-a.pti_total_system,ru=ru_us(rb.ru_utime)-ru_us(ra.ru_utime),rs=ru_us(rb.ru_stime)-ru_us(ra.ru_stime);double ratio_raw=(double)(pu+ps)/((ru+rs)*1000.0),ratio_mach=ratio_raw*tb.numer/tb.denom;
 printf("%s{\"round\":%d,\"wall_ns\":%"PRIu64",\"proc_user_raw\":%"PRIu64",\"proc_system_raw\":%"PRIu64",\"rusage_user_us\":%"PRIu64",\"rusage_system_us\":%"PRIu64",\"raw_to_rusage_ns_ratio\":%.8f,\"mach_converted_to_rusage_ns_ratio\":%.8f,\"resident_bytes\":%"PRIu64"}",round?",":"",round,wb-wa,pu,ps,ru,rs,ratio_raw,ratio_mach,b.pti_resident_size);
 if(!((ratio_raw>=0.8&&ratio_raw<=1.2)||(ratio_mach>=0.8&&ratio_mach<=1.2)))return 4;
 }
 printf("]}\n");(void)sink;return 0;}
