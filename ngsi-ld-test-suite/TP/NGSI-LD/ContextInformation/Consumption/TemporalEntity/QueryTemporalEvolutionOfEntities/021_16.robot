*** Settings ***
Documentation       Check temporal pagination is applied when querying the temporal evolution of entities via POST

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Suite Setup         Setup Initial Entities
Suite Teardown      Delete Initial Entities
Test Template       Retrieve Temporal Entities Via Post


*** Variables ***
${first_vehicle_payload_file}=      pagination/2020-01-vehicule-temporal-representation-twenty-instances.jsonld
${second_vehicle_payload_file}=     2020-09-vehicle-temporal-representation.jsonld


*** Test Cases ***    PAYLOAD_FILE
021_16_01 Retrieve The Entities Via Post
    [Tags]    te-retrieve    5_7_3    6_3_10    since_v1.5.1
    entity-operations-before-query.jsonld


*** Keywords ***
Retrieve Temporal Entities Via Post
    [Documentation]    Check that temporal pagination is triggered on the post temporal query
    [Arguments]    ${payload_file}

    ${response}=    Query Temporal Representation Of Entities Via Post
    ...    query_file_name=${payload_file}
    ...    context=${ngsild_test_suite_context}

    Check Response Status Code    206    ${response.status_code}

    ${contentRange}=    Get Regexp Matches
    ...    ${response.headers}[Content-Range]
    ...    ([a-zA-Z\-]+) (.*-.*-.*)-(.*-.*-.*)\/(.*)
    ...    1
    ...    2
    ...    3
    ...    4
    ${unit}=    Set Variable    ${contentRange}[0][0]

    Check Content Range Part Equal    ${unit}    date-time

Setup Initial Entities
    ${first_temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${second_temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${response}=    Create Temporal Representation Of Entity
    ...    ${first_vehicle_payload_file}
    ...    ${first_temporal_entity_representation_id}
    Check Response Status Code    201    ${response.status_code}

    ${response}=    Create Temporal Representation Of Entity
    ...    ${second_vehicle_payload_file}
    ...    ${second_temporal_entity_representation_id}
    Check Response Status Code    201    ${response.status_code}

    Set Suite Variable    ${first_temporal_entity_representation_id}
    Set Suite Variable    ${second_temporal_entity_representation_id}

Delete Initial Entities
    Delete Temporal Representation Of Entity    ${first_temporal_entity_representation_id}
    Delete Temporal Representation Of Entity    ${second_temporal_entity_representation_id}
