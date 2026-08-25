*** Settings ***
Documentation       Check that one can retrieve the temporal evolution of an entity following the deletedAt temporal property

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
${vehicle_payload_file}=    vehicle-temporal-representation-different-attributes-types.jsonld


*** Test Cases ***    ATTR_NAME    EXPECTATION_FILENAME
020_17_01 With A Property
    [Tags]    te-retrieve    5_7_3    since_v1.6.1
    fuelLevel    vehicle-temporal-representation-property-020-17.jsonld
020_17_02 With A Relationship
    [Tags]    te-retrieve    5_7_3    since_v1.6.1
    isParkedIn    vehicle-temporal-representation-relationship-020-17.jsonld
020_17_03 With A LanguageProperty
    [Tags]    te-retrieve    5_7_3    4_5_18    since_v1.6.1
    name    vehicle-temporal-representation-languageproperty-020-17.jsonld


*** Keywords ***
Delete Temporal Attribute and Retrieve Temporal Evolution of Attribute
    [Documentation]    Check that one can retrieve the temporal evolution of an entity following the deletedAt temporal property
    [Arguments]    ${attr_name}    ${expectation_filename}
    ${response}=    Delete Entity Attributes
    ...    entityId=${temporal_entity_representation_id}
    ...    attributeId=${attr_name}
    ...    datasetId=${EMPTY}
    ...    deleteAll=false
    ...    context=${ngsild_test_suite_context}

    ${response2}=    Retrieve Temporal Representation Of Entity
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    timeproperty=deletedAt
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response2.status_code}
    Check Response Body Containing EntityTemporal element
    ...    filename=${expectation_filename}
    ...    temporal_entity_representation_id=${temporal_entity_representation_id}
    ...    response_body=${response2.json()}
    ...    excluded_regex=deletedAt
    Should Have Value In Json
    ...    json_object=${response2.json()}
    ...    json_path=['${attr_name}'][0]['deletedAt']

Create Temporal Entity
    ${temporal_entity_representation_id}=    Generate Random Vehicle Entity Id
    ${create_response}=    Create Temporal Representation Of Entity
    ...    ${vehicle_payload_file}
    ...    ${temporal_entity_representation_id}
    Check Response Status Code    201    ${create_response.status_code}
    Set Suite Variable    ${temporal_entity_representation_id}

Delete Initial Temporal Entity
    Delete Temporal Representation Of Entity    ${temporal_entity_representation_id}
