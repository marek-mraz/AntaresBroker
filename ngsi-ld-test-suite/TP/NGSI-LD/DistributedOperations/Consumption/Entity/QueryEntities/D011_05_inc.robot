*** Settings ***
Documentation       Verify that Entity ordering is refused when the query would execute as
...                 a distributed operation, and permitted once it is limited to local
...                 scope.
...
...                 5.7.2.4: "If the ordering parameter is present and the execution of the
...                 operation is not limited to the local scope (see clause 5.5.13) then an
...                 error of type BadRequestData shall be raised."
...                 4.23.1: "Sort ordering is never applied to distributed operations", and
...                 scopes ordering to "Entities retrieved from a single context broker".
...
...                 The trigger is the EXECUTION, not the presence of the parameter: with a
...                 matching registration in place the query fans out, so orderBy must be
...                 refused; `local=true` brings it back into scope. The purely local case
...                 (no registration ⇒ ordering allowed) is pinned by
...                 ContextInformation/Consumption/Entity/QueryEntities/019_27.
...
...                 Antares extension TP — 4.23 has only happy-path official coverage.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Suite Setup         Setup Matching Registration
Suite Teardown      Delete Context Source Registration    ${registration_id}


*** Variables ***
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-complete.jsonld


*** Test Cases ***
D011_05_01_inc Ordering Is Refused For A Distributed Query
    [Documentation]    A registration matches type=Vehicle, so the query is distributed and
    ...    orderBy must raise BadRequestData
    [Tags]    dist-ops    4_23    5_7_2    additive-inclusive    since_v1.9.1

    ${response}=    Query Entities
    ...    entity_types=Vehicle
    ...    orderBy=brandName
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}

D011_05_02_inc Ordering Is Permitted Once Limited To Local Scope
    [Documentation]    local=true limits execution to this broker, so 4.23 ordering applies
    [Tags]    dist-ops    4_23    5_7_2    5_5_13    additive-inclusive    since_v1.9.1

    ${response}=    Query Entities
    ...    entity_types=Vehicle
    ...    orderBy=brandName
    ...    local=true
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}

D011_05_03_inc A Distributed Query Without Ordering Is Unaffected
    [Documentation]    The guard is specific to the ordering parameter
    [Tags]    dist-ops    4_23    5_7_2    additive-inclusive    since_v1.9.1

    ${response}=    Query Entities
    ...    entity_types=Vehicle
    ...    local=true
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    200    ${response.status_code}


*** Keywords ***
Setup Matching Registration
    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ${response}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response.status_code}
