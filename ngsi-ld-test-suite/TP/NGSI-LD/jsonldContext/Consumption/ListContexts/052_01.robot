*** Settings ***
Documentation       Check that one can list all the @context available in the broker with no previous add @context

Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Template       List @contexts with no previous created @context


*** Variables ***
${filename}=        @context-minimal-valid.json
${reason_200}=      OK
${reason_204}=      No Content


*** Test Cases ***    DETAILS    KIND
052_01_01 List @contexts With Neither Details Or Kind And Not Previously Created @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    ${EMPTY}    ${EMPTY}
052_01_02 List @contexts With No Details And Kind Equal To Hosted And Not Previously Created @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    ${EMPTY}    Hosted
052_01_03 List @contexts With No Details And Kind Equal To Cached And Not Previously Created @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    ${EMPTY}    Cached
052_01_04 List @contexts With No Details And Kind Equal To Implicitlycreated And Not Previously Created @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    ${EMPTY}    ImplicitlyCreated
052_01_05 List @contexts With Details Equal To False And No Kind And Not Previously Created @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    false    ${EMPTY}
052_01_06 List @contexts With Details Equal To False And Kind Equal To Hosted And Not Previously Created @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    false    Hosted
052_01_07 List @contexts With Details Equal To False And Kind Equal To Cached And Not Previously Created @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    false    Cached
052_01_08 List @contexts With Details Equal To False And Kind Equal To Implicitlycreated And Not Previously Created @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    false    ImplicitlyCreated


*** Keywords ***
List @contexts with no previous created @context
    [Documentation]    Check that one can list @contexts
    [Arguments]    ${details}    ${kind}
    ${response}=    List @contexts    ${details}    ${kind}
    ${empty_array}=    Create List
    Check Response Status Code    200    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason_200}
    Check Context Response Body Containing a list of identifiers
    ...    response_body=${response.json()}
    ...    list_contexts=${empty_array}
