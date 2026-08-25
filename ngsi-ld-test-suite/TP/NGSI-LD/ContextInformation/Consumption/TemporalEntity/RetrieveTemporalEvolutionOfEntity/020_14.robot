*** Settings ***
Documentation       Check that the time range cut before the second attribute to avoid missing data in content-range

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Create Temporal Entity
Suite Teardown      Delete Initial Temporal Entity
Test Template       Retrieve Temporal Entity


*** Variables ***
${vehicle_payload_file}=    pagination/2020-01-vehicule-temporal-representation-sixty-instances.jsonld
${timeBefore}=              2019-01-01T01:01:00Z
${timeAfter}=               2021-01-01T01:01:00Z


*** Test Cases ***
020_14_01 Retrieve An Entity With 60 Instances Of Unsynchronized Attributes
    [Tags]    te-retrieve    5_7_3    6_3_10    since_v1.5.1
    timerel=after    timeAt=${timeBefore}    emptyAttr=fuelLevel
020_14_02 Retrieve The Entity With lastN
    [Tags]    te-retrieve    5_7_3    6_3_10    since_v1.5.1
    lastN=${100}    timerel=before    timeAt=${timeAfter}    emptyAttr=speed


*** Keywords ***
Retrieve Temporal Entity
    [Documentation]    Check that the time range cut before the second attribute
    [Arguments]
    ...    ${timerel}
    ...    ${timeAt}
    ...    ${emptyAttr}
    ...    ${lastN}=${EMPTY}
    ${response}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    context=${ngsild_test_suite_context}
    ...    timerel=${timerel}
    ...    timeAt=${timeAt}
    ...    lastN=${lastN}
    Check Data Is Empty    ${response.json()}[${emptyAttr}]

Create Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${response}=    Create Temporal Representation Of Entity
    ...    ${vehicle_payload_file}
    ...    ${temporal_entity_representation_id}
    Check Response Status Code    201    ${response.status_code}

    Set Suite Variable    ${temporal_entity_representation_id}

Delete Initial Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
