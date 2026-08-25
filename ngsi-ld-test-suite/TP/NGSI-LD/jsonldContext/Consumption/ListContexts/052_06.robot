*** Settings ***
Documentation       Check that one can list all the @context available in the broker with no previous add @context

Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationSubscription.resource
Resource            ${EXECDIR}/resources/ApiUtils/jsonldContext.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Setup          Create initial ImplicitlyCreated @context
Test Teardown       Delete Initial @context Data
Test Template       List @contexts with no previous created @context


*** Variables ***
${subscription_payload_file_path}=      jsonldContext/subscription-with-implicitlycreated-contexts.jsonld
${filename}=                            @context-minimal-valid.json
${reason_200}=                          OK
${reason_204}=                          No Content


*** Test Cases ***    DETAILS    KIND
052_06_01 List @contexts With Neither Details Or Kind And A Created ImplicitlyCreated @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    ${EMPTY}    ${EMPTY}
052_06_02 List @contexts With No Details And Kind Set To Hosted And A Created ImplicitlyCreated @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    ${EMPTY}    Hosted
052_06_03 List @contexts With No Details And Kind Set To Cached And A Created ImplicitlyCreated @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    ${EMPTY}    Cached
052_06_04 List @contexts With No Details And Kind Set To Implicitlycreated And A Created ImplicitlyCreated @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    ${EMPTY}    ImplicitlyCreated
052_06_05 List @contexts With Details Set To False And No Kind And A Created ImplicitlyCreated @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    false    ${EMPTY}
052_06_06 List @contexts With Details Set To False And Kind Equal To Hosted And A Created ImplicitlyCreated @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    false    Hosted
052_06_07 List @contexts With Details Set To False And Kind Equal To Cached And A Created ImplicitlyCreated @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    false    Cached
052_06_08 List @contexts With Details Set To False And Kind Equal To Implicitlycreated And A Created ImplicitlyCreated @context
    [Tags]    ctx-list    5_13_3    since_v1.5.1
    false    ImplicitlyCreated


*** Keywords ***
List @contexts with no previous created @context
    [Documentation]    Check that one can list @contexts
    [Arguments]    ${details}    ${kind}
    ${response}=    List @contexts    ${details}    ${kind}

    Check Response Status Code    200    ${response.status_code}
    Check Response Reason set to    ${response.reason}    ${reason_200}

    IF    '${kind}' == 'Hosted'
        ${entryFound}=    Run Keyword And Return Status
        ...    Check Context Response Body Containing a list of identifiers    ${response.json()}
        ...        ${list_contexts}
        ...        ${kind}
        Should Not Be True    ${entryFound}
    ELSE IF    '${kind}' == 'ImplicitlyCreated'
        ${tmp}=    Create List    ${implicit_id}
        Check Context Response Body Containing a list of identifiers
        ...    ${response.json()}
        ...    ${tmp}
        ...    ${kind}
    ELSE IF    '${kind}' == 'Cached' or '{$kind}' == ''
        Check Context Response Body Containing a list of identifiers
        ...    ${response.json()}
        ...    ${list_contexts}
        ...    ${kind}
    END

Create initial ImplicitlyCreated @context
    ${subscription_payload}=    Load JSON From File    ${EXECDIR}/data/${subscription_payload_file_path}

    ${subscription_id}=    Get Value From JSON    ${subscription_payload}    $..id
    ${subscription_id}=    Get From List    ${subscription_id}    0

    ${list_contexts}=    Get Value From JSON    ${subscription_payload}    $..@context
    ${list_contexts}=    Get From List    ${list_contexts}    0

    ${response}=    Create Subscription
    ...    ${subscription_id}
    ...    ${subscription_payload_file_path}
    ...    ${CONTENT_TYPE_LD_JSON}

    ${response}=    Retrieve Subscription    ${subscription_id}
    ${implicit_id}=    Get From Dictionary    ${response.json()}    jsonldContext
    ${implicit_id}=    Evaluate    '${implicit_id}'.split('/')[-1]

    Set Global Variable    ${implicit_id}
    Set Suite Variable    ${subscription_id}
    Set Suite Variable    ${list_contexts}
    ${response}=    Serve a @context    ${implicit_id}
    Check Response Status Code    200    ${response.status_code}

    ${data}=    Set Variable    ${response.json()}

    Check Context Response Body Containing numberOfHits value    ${data}    1

Delete Initial @context Data
    Delete Subscription    ${subscription_id}
    FOR    ${uri}    IN    @{list_contexts}
        Log    URI: ${uri}
        Delete a @context    ${uri}
    END
