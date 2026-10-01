#include <libproc.h>
#include <sys/proc_info.h>
#include <stddef.h>
#include <stdio.h>
#include <unistd.h>
#include <pthread.h>
#include <stdatomic.h>
#include <time.h>
static atomic_int started=0,release_workers=0;
static void *worker(void *unused){(void)unused;atomic_fetch_add(&started,1);while(!atomic_load(&release_workers)){struct timespec s={0,1000000};nanosleep(&s,0);}return 0;}
static int sample(void){struct proc_taskinfo task={0};int n=proc_pidinfo(getpid(),PROC_PIDTASKINFO,0,&task,sizeof(task));return n==sizeof(task)?task.pti_threadnum:-1;}
int main(void){int before=sample();pthread_t threads[6];for(int i=0;i<6;i++){if(pthread_create(&threads[i],0,worker,0))return 2;}while(atomic_load(&started)!=6){}int active=sample();atomic_store(&release_workers,1);for(int i=0;i<6;i++)pthread_join(threads[i],0);int after=sample();printf("{\"sdk_size\":%zu,\"threadnum_offset\":%zu,\"sdk_flavor\":%d,\"before\":%d,\"during_six_threads\":%d,\"after\":%d}\n",sizeof(struct proc_taskinfo),offsetof(struct proc_taskinfo,pti_threadnum),PROC_PIDTASKINFO,before,active,after);return sizeof(struct proc_taskinfo)==96&&offsetof(struct proc_taskinfo,pti_threadnum)==84&&before>0&&active>=before+6&&after==before?0:1;}
