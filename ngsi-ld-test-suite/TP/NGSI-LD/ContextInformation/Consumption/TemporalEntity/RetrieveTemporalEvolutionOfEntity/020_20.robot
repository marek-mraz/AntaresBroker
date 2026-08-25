*** Settings ***
Documentation       Check that one can retrieve the temporal evolution of a scope following the deletedAt temporal property with temporal values

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Temporal Entity
Test Teardown       Delete Initial Temporal Entity
Test Template       Delete Temporal Attribute and Retrieve Temporal Evolution of Attribute


*** Variables ***
${vehicle_payload_file}=    vehicle-temporal-representation-scope.jsonld


*** Test Cases ***    EXPECTATION_FILENAME
020_20_01 With A Scope
    [Tags]    te-retrieve    4_18    5_7_3    6_19_3_1    since_v1.6.1
    vehicle-temporal-representation-property-020-20.jsonld


*** Keywords ***
Delete Temporal Attribute and Retrieve Temporal Evolution of Attribute
    [Documentation]    Check that one can retrieve the temporal evolution of a scope following the deletedAt temporal property with temporal values
    [Arguments]    ${expectation_filename}
    ${response}=    Delete Entity Attributes
    ...    entityId=${temporal_entity_representation_id}
    ...    attributeId=scope
    ...    datasetId=${EMPTY}
    ...    deleteAll=false
    ...    context=${ngsild_test_suite_context}

    ${response2}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    timeproperty=deletedAt
    ...    options=temporalValues
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response2.status_code}
    Check Response Body Containing EntityTemporal element
    ...    filename=${expectation_filename}
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    response_body=${response2.json()}
    ...    excluded_regex=root\\['scope'\\]\\['values'\\]\\[0\\]\\[1\\]
    Should Have Value In Json
    ...    json_object=${response2.json()}
    ...    json_path=['scope']['values'][0][1]

Create Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${create_response}=    Create Temporal Representation Of Entity
    ...    ${vehicle_payload_file}
    ...    ${temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${temporal_entity_representation_id}

Delete Initial Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
